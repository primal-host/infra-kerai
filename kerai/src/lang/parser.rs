use std::collections::HashMap;

use super::ast::{Document, Line, Notation};
use super::expr::Expr;
use super::pratt;
use super::token::{tokenize, Token, TokenKind};

/// Returns true if the token value is a known binary operator.
fn is_known_binary_op(s: &str) -> bool {
    matches!(s, "+" | "-" | "*" | "/" | "%")
}

/// Parser with a notation mode stack and alias tracking for `.kerai` files.
pub struct Parser {
    notation_stack: Vec<Notation>,
    aliases: HashMap<String, String>,
}

impl Parser {
    pub fn new() -> Self {
        Parser {
            notation_stack: vec![Notation::Postfix],
            aliases: HashMap::new(),
        }
    }

    /// Current notation mode (top of stack).
    fn notation(&self) -> Notation {
        *self.notation_stack.last().unwrap_or(&Notation::Postfix)
    }

    /// Replace the current notation mode (mutate top of stack).
    fn set_notation(&mut self, notation: Notation) {
        if let Some(top) = self.notation_stack.last_mut() {
            *top = notation;
        }
    }

    /// Push a notation mode onto the stack (for scoped paren groups).
    pub fn push_notation(&mut self, notation: Notation) {
        self.notation_stack.push(notation);
    }

    /// Pop a notation mode from the stack. Never pops below depth 1.
    pub fn pop_notation(&mut self) {
        if self.notation_stack.len() > 1 {
            self.notation_stack.pop();
        }
    }

    /// Resolve a potential kerai directive (e.g., `k.postfix` → `kerai.postfix`).
    /// Returns the fully-qualified directive if recognized, None otherwise.
    fn resolve_directive(&self, value: &str) -> Option<String> {
        if value.starts_with("kerai.") {
            return Some(value.to_string());
        }
        if let Some((prefix, suffix)) = value.split_once('.') {
            if self.aliases.get(prefix).map(|v| v.as_str()) == Some("kerai") {
                return Some(format!("kerai.{suffix}"));
            }
        }
        None
    }

    /// Parse a complete document from source text.
    pub fn parse(&mut self, source: &str) -> Document {
        let mut doc = Document::new();

        for line in source.lines() {
            let parsed = self.parse_line(line);

            // Track definitions for alias resolution
            if let Line::Definition { ref name, ref target, .. } = parsed {
                self.aliases.insert(name.clone(), target.clone());
            }

            doc.lines.push(parsed);
        }

        doc.default_notation = self.notation();
        doc
    }

    /// Parse a single line into a `Line` variant.
    fn parse_line(&mut self, raw: &str) -> Line {
        let trimmed = raw.trim();

        // Empty line
        if trimmed.is_empty() {
            return Line::Empty;
        }

        // Comment line
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            return Line::Comment {
                text: raw.to_string(),
            };
        }

        let tokens = tokenize(trimmed);
        if tokens.is_empty() {
            return Line::Empty;
        }

        // Definition line: `name: target` — trailing colon on first token
        if tokens[0].value.ends_with(':') && !tokens[0].quoted && tokens.len() >= 2 {
            let name = tokens[0].value.trim_end_matches(':').to_string();
            let target = tokens[1..]
                .iter()
                .map(|t| t.value.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            return Line::Definition {
                name,
                target,
                notation: self.notation(),
            };
        }

        // Check for kerai.* directive (or alias-resolved equivalent)
        if !tokens[0].quoted {
            if let Some(resolved) = self.resolve_directive(&tokens[0].value) {
                let args: Vec<String> = tokens[1..].iter().map(|t| t.value.clone()).collect();

                // Apply side effect: notation mode change
                match resolved.as_str() {
                    "kerai.prefix" => self.set_notation(Notation::Prefix),
                    "kerai.infix" => self.set_notation(Notation::Infix),
                    "kerai.postfix" => self.set_notation(Notation::Postfix),
                    _ => {}
                }

                return Line::Directive {
                    name: tokens[0].value.clone(),
                    args,
                };
            }
        }

        // All lines go through the expression parser for their notation mode
        let notation = self.notation();
        self.parse_expr_line(&tokens, notation)
    }

    /// Parse a line with expression-aware parsing (handles parens, brackets, and operators).
    fn parse_expr_line(&mut self, tokens: &[Token], notation: Notation) -> Line {
        let expr = match notation {
            Notation::Infix => {
                match pratt::parse_infix(tokens) {
                    Some(e) => e,
                    None => return Line::Empty,
                }
            }
            Notation::Prefix => self.parse_prefix_expr(tokens),
            Notation::Postfix => self.parse_postfix_expr(tokens),
        };

        match expr {
            Expr::Atom(s) => Line::Call {
                function: s,
                args: vec![],
                notation,
            },
            Expr::Apply { function, args } => Line::Call {
                function,
                args,
                notation,
            },
            Expr::List(elements) => Line::Call {
                function: "list".into(),
                args: vec![Expr::List(elements)],
                notation,
            },
        }
    }

    /// Stack-based postfix expression parser.
    ///
    /// Known binary operators (`+`, `-`, `*`, `/`, `%`) pop two operands from
    /// the stack. Everything else pushes as an operand. If no known operator
    /// fires, falls back to flat postfix (last = function, rest = args).
    fn parse_postfix_expr(&mut self, tokens: &[Token]) -> Expr {
        let mut stack: Vec<Expr> = Vec::new();
        let mut any_op_fired = false;
        let mut i = 0;

        while i < tokens.len() {
            match tokens[i].kind {
                TokenKind::LParen => {
                    if let Some((inner, end)) = extract_paren_group(tokens, i) {
                        let expr = self.parse_paren_group(inner, Notation::Postfix);
                        stack.push(expr);
                        i = end + 1;
                    } else {
                        stack.push(Expr::Atom(tokens[i].value.clone()));
                        i += 1;
                    }
                }
                TokenKind::LBracket => {
                    if let Some((inner, end)) = extract_bracket_group(tokens, i) {
                        let elements = self.parse_list_elements(inner);
                        stack.push(Expr::List(elements));
                        i = end + 1;
                    } else {
                        stack.push(Expr::Atom(tokens[i].value.clone()));
                        i += 1;
                    }
                }
                TokenKind::Word => {
                    if !tokens[i].quoted && is_known_binary_op(&tokens[i].value) && stack.len() >= 2 {
                        let b = stack.pop().unwrap();
                        let a = stack.pop().unwrap();
                        stack.push(Expr::Apply {
                            function: tokens[i].value.clone(),
                            args: vec![a, b],
                        });
                        any_op_fired = true;
                    } else {
                        stack.push(Expr::Atom(tokens[i].value.clone()));
                    }
                    i += 1;
                }
                TokenKind::RParen | TokenKind::RBracket => {
                    i += 1; // skip unexpected closing delimiters
                }
            }
        }

        if stack.is_empty() {
            return Expr::Atom(String::new());
        }
        if stack.len() == 1 {
            return stack.into_iter().next().unwrap();
        }

        // If no known operator fired, use flat fallback: last = function, rest = args
        if !any_op_fired {
            let last = stack.pop().unwrap();
            match last {
                Expr::Atom(function) => Expr::Apply {
                    function,
                    args: stack,
                },
                Expr::Apply { function, args: inner_args } => {
                    let mut all_args = stack;
                    all_args.extend(inner_args);
                    Expr::Apply { function, args: all_args }
                }
                Expr::List(_) => {
                    // Last item is a list — no clear function, return last on stack
                    stack.push(last);
                    let function_expr = stack.pop().unwrap();
                    function_expr
                }
            }
        } else {
            // Known ops fired — remaining stack items form the result
            // If multiple items remain, treat as nested applications
            if stack.len() == 1 {
                stack.into_iter().next().unwrap()
            } else {
                // Multiple remaining — last is result
                stack.pop().unwrap()
            }
        }
    }

    /// Recursive prefix expression parser.
    ///
    /// Known binary operators consume two arguments recursively.
    /// Non-operator words are atoms. If no known operator is found in the
    /// entire expression, falls back to flat prefix (first = function, rest = args).
    fn parse_prefix_expr(&mut self, tokens: &[Token]) -> Expr {
        // Check if any word token at depth 0 is a known binary op
        let mut depth = 0;
        let mut has_known_op = false;
        let mut has_groups = false;
        for t in tokens {
            match t.kind {
                TokenKind::LParen | TokenKind::LBracket => {
                    has_groups = true;
                    depth += 1;
                }
                TokenKind::RParen | TokenKind::RBracket => {
                    depth -= 1;
                }
                TokenKind::Word if depth == 0 && !t.quoted && is_known_binary_op(&t.value) => {
                    has_known_op = true;
                }
                _ => {}
            }
        }

        if has_known_op {
            // Use recursive descent prefix parsing
            let mut pos = 0;
            let result = self.parse_prefix_recursive(tokens, &mut pos);
            // If there are remaining tokens after the prefix parse, they're extra
            // args — this shouldn't normally happen with well-formed prefix
            result
        } else if has_groups {
            // Has paren/bracket groups but no known operators — flat prefix with groups
            let exprs = self.parse_arg_list(tokens, Notation::Prefix);
            self.flat_prefix_from_exprs(exprs)
        } else {
            // Pure flat prefix: first = function, rest = args
            let exprs: Vec<Expr> = tokens.iter().map(|t| Expr::Atom(t.value.clone())).collect();
            self.flat_prefix_from_exprs(exprs)
        }
    }

    /// Convert a list of expressions into flat prefix form (first = function, rest = args).
    fn flat_prefix_from_exprs(&self, exprs: Vec<Expr>) -> Expr {
        if exprs.is_empty() {
            return Expr::Atom(String::new());
        }
        if exprs.len() == 1 {
            return exprs.into_iter().next().unwrap();
        }
        let mut iter = exprs.into_iter();
        let first = iter.next().unwrap();
        match first {
            Expr::Atom(function) => Expr::Apply {
                function,
                args: iter.collect(),
            },
            Expr::Apply { function, mut args } => {
                args.extend(iter);
                Expr::Apply { function, args }
            }
            other => other,
        }
    }

    /// Recursive prefix parser — known binary ops consume two args.
    fn parse_prefix_recursive(&mut self, tokens: &[Token], pos: &mut usize) -> Expr {
        if *pos >= tokens.len() {
            return Expr::Atom(String::new());
        }

        match tokens[*pos].kind {
            TokenKind::LParen => {
                if let Some((inner, end)) = extract_paren_group(tokens, *pos) {
                    *pos = end + 1;
                    self.parse_paren_group(inner, Notation::Prefix)
                } else {
                    let val = tokens[*pos].value.clone();
                    *pos += 1;
                    Expr::Atom(val)
                }
            }
            TokenKind::LBracket => {
                if let Some((inner, end)) = extract_bracket_group(tokens, *pos) {
                    *pos = end + 1;
                    let elements = self.parse_list_elements(inner);
                    Expr::List(elements)
                } else {
                    let val = tokens[*pos].value.clone();
                    *pos += 1;
                    Expr::Atom(val)
                }
            }
            TokenKind::Word => {
                let val = tokens[*pos].value.clone();
                *pos += 1;
                if !tokens[*pos - 1].quoted && is_known_binary_op(&val) {
                    let a = self.parse_prefix_recursive(tokens, pos);
                    let b = self.parse_prefix_recursive(tokens, pos);
                    Expr::Apply {
                        function: val,
                        args: vec![a, b],
                    }
                } else {
                    Expr::Atom(val)
                }
            }
            TokenKind::RParen | TokenKind::RBracket => {
                *pos += 1;
                Expr::Atom(String::new())
            }
        }
    }

    /// Parse a token slice into a list of `Expr` values, handling paren and bracket groups.
    fn parse_arg_list(&mut self, tokens: &[Token], notation: Notation) -> Vec<Expr> {
        let mut args = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            match tokens[i].kind {
                TokenKind::LParen => {
                    if let Some((inner, end)) = extract_paren_group(tokens, i) {
                        let expr = self.parse_paren_group(inner, notation);
                        args.push(expr);
                        i = end + 1;
                    } else {
                        args.push(Expr::Atom(tokens[i].value.clone()));
                        i += 1;
                    }
                }
                TokenKind::LBracket => {
                    if let Some((inner, end)) = extract_bracket_group(tokens, i) {
                        let elements = self.parse_list_elements(inner);
                        args.push(Expr::List(elements));
                        i = end + 1;
                    } else {
                        args.push(Expr::Atom(tokens[i].value.clone()));
                        i += 1;
                    }
                }
                TokenKind::RParen | TokenKind::RBracket => {
                    i += 1;
                }
                TokenKind::Word => {
                    args.push(Expr::Atom(tokens[i].value.clone()));
                    i += 1;
                }
            }
        }

        args
    }

    /// Parse the contents of a bracket group as list elements (no operator evaluation).
    fn parse_list_elements(&mut self, tokens: &[Token]) -> Vec<Expr> {
        let mut elements = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            match tokens[i].kind {
                TokenKind::LBracket => {
                    if let Some((inner, end)) = extract_bracket_group(tokens, i) {
                        let nested = self.parse_list_elements(inner);
                        elements.push(Expr::List(nested));
                        i = end + 1;
                    } else {
                        elements.push(Expr::Atom(tokens[i].value.clone()));
                        i += 1;
                    }
                }
                TokenKind::LParen => {
                    if let Some((inner, end)) = extract_paren_group(tokens, i) {
                        // Inside a list, paren groups are still parsed as expressions
                        let expr = self.parse_paren_group(inner, Notation::Prefix);
                        elements.push(expr);
                        i = end + 1;
                    } else {
                        elements.push(Expr::Atom(tokens[i].value.clone()));
                        i += 1;
                    }
                }
                TokenKind::RParen | TokenKind::RBracket => {
                    i += 1;
                }
                TokenKind::Word => {
                    elements.push(Expr::Atom(tokens[i].value.clone()));
                    i += 1;
                }
            }
        }

        elements
    }

    /// Parse the contents of a parenthesized group.
    /// Checks for a scoped notation directive as the first token.
    fn parse_paren_group(&mut self, inner: &[Token], default_notation: Notation) -> Expr {
        if inner.is_empty() {
            return Expr::Atom(String::new());
        }

        // Check first token for scoped directive
        if !inner[0].quoted && inner[0].kind == TokenKind::Word {
            if let Some(resolved) = self.resolve_directive(&inner[0].value) {
                if let Some(notation) = directive_to_notation(&resolved) {
                    self.push_notation(notation);
                    let expr = self.parse_tokens_as_expr(&inner[1..], notation);
                    self.pop_notation();
                    return expr;
                }
            }
        }

        // No directive — parse under the given notation
        self.parse_tokens_as_expr(inner, default_notation)
    }

    /// Parse a token slice as a single expression under a given notation mode.
    fn parse_tokens_as_expr(&mut self, tokens: &[Token], notation: Notation) -> Expr {
        if tokens.is_empty() {
            return Expr::Atom(String::new());
        }

        match notation {
            Notation::Infix => pratt::parse_infix(tokens).unwrap_or(Expr::Atom(String::new())),
            Notation::Prefix => self.parse_prefix_expr(tokens),
            Notation::Postfix => self.parse_postfix_expr(tokens),
        }
    }
}

/// Extract the inner tokens of a balanced paren group starting at `start`.
/// Returns `(inner_tokens_slice, closing_paren_index)`.
fn extract_paren_group(tokens: &[Token], start: usize) -> Option<(&[Token], usize)> {
    if tokens.get(start)?.kind != TokenKind::LParen {
        return None;
    }
    let mut depth = 0;
    for (i, tok) in tokens[start..].iter().enumerate() {
        match tok.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => {
                depth -= 1;
                if depth == 0 {
                    let end = start + i;
                    return Some((&tokens[start + 1..end], end));
                }
            }
            _ => {}
        }
    }
    None // unmatched
}

/// Extract the inner tokens of a balanced bracket group starting at `start`.
/// Returns `(inner_tokens_slice, closing_bracket_index)`.
fn extract_bracket_group(tokens: &[Token], start: usize) -> Option<(&[Token], usize)> {
    if tokens.get(start)?.kind != TokenKind::LBracket {
        return None;
    }
    let mut depth = 0;
    for (i, tok) in tokens[start..].iter().enumerate() {
        match tok.kind {
            TokenKind::LBracket => depth += 1,
            TokenKind::RBracket => {
                depth -= 1;
                if depth == 0 {
                    let end = start + i;
                    return Some((&tokens[start + 1..end], end));
                }
            }
            _ => {}
        }
    }
    None // unmatched
}

/// Map a fully-qualified kerai directive to a notation mode.
fn directive_to_notation(directive: &str) -> Option<Notation> {
    match directive {
        "kerai.prefix" => Some(Notation::Prefix),
        "kerai.infix" => Some(Notation::Infix),
        "kerai.postfix" => Some(Notation::Postfix),
        _ => None,
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_comment_lines() {
        let mut parser = Parser::new();
        let doc = parser.parse("# a comment\n\n// another comment\n");
        assert_eq!(doc.lines.len(), 3);
        assert!(matches!(&doc.lines[0], Line::Comment { text } if text == "# a comment"));
        assert!(matches!(&doc.lines[1], Line::Empty));
        assert!(matches!(&doc.lines[2], Line::Comment { text } if text == "// another comment"));
    }

    #[test]
    fn definition_line() {
        let mut parser = Parser::new();
        let doc = parser.parse("pg: postgres\n");
        assert_eq!(doc.lines.len(), 1);
        match &doc.lines[0] {
            Line::Definition {
                name,
                target,
                notation,
            } => {
                assert_eq!(name, "pg");
                assert_eq!(target, "postgres");
                assert_eq!(*notation, Notation::Postfix);
            }
            other => panic!("expected Definition, got {other:?}"),
        }
    }

    #[test]
    fn prefix_call() {
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.prefix\npostgres.global.connection localhost\n");
        assert_eq!(doc.lines.len(), 2);
        match &doc.lines[1] {
            Line::Call {
                function,
                args,
                notation,
            } => {
                assert_eq!(function, "postgres.global.connection");
                assert_eq!(args, &[Expr::Atom("localhost".into())]);
                assert_eq!(*notation, Notation::Prefix);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn infix_mode_flat() {
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.infix\na b c\n");
        assert_eq!(doc.lines.len(), 2);
        assert!(matches!(&doc.lines[0], Line::Directive { name, .. } if name == "kerai.infix"));
        match &doc.lines[1] {
            Line::Call {
                function,
                args,
                notation,
            } => {
                // In infix mode with Pratt parser, `a b c` is parsed as:
                // `a` is atom, `b` is unknown operator (bp 5,6), `c` is atom
                // → Apply("b", [Atom("a"), Atom("c")])
                assert_eq!(function, "b");
                assert_eq!(args, &[Expr::Atom("a".into()), Expr::Atom("c".into())]);
                assert_eq!(*notation, Notation::Infix);
            }
            other => panic!("expected infix Call, got {other:?}"),
        }
    }

    #[test]
    fn postfix_mode() {
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.postfix\na b c\n");
        assert_eq!(doc.lines.len(), 2);
        match &doc.lines[1] {
            Line::Call {
                function,
                args,
                notation,
            } => {
                assert_eq!(function, "c");
                assert_eq!(args, &[Expr::Atom("a".into()), Expr::Atom("b".into())]);
                assert_eq!(*notation, Notation::Postfix);
            }
            other => panic!("expected postfix Call, got {other:?}"),
        }
    }

    #[test]
    fn notation_switch_midfile() {
        let mut parser = Parser::new();
        let doc = parser.parse("a b\nkerai.infix\nc d\nkerai.prefix\ne f\n");
        // Line 0: postfix call b(a) — default is now postfix
        assert!(
            matches!(&doc.lines[0], Line::Call { function, notation, .. } if function == "b" && *notation == Notation::Postfix)
        );
        // Line 1: directive
        assert!(matches!(&doc.lines[1], Line::Directive { .. }));
        // Line 2: infix call d(c) — Pratt parser treats d as unknown operator
        assert!(
            matches!(&doc.lines[2], Line::Call { function, notation, .. } if function == "d" && *notation == Notation::Infix)
        );
        // Line 3: directive
        assert!(matches!(&doc.lines[3], Line::Directive { .. }));
        // Line 4: prefix call e(f)
        assert!(
            matches!(&doc.lines[4], Line::Call { function, notation, .. } if function == "e" && *notation == Notation::Prefix)
        );
    }

    #[test]
    fn single_token_call() {
        let mut parser = Parser::new();
        let doc = parser.parse("ping\n");
        match &doc.lines[0] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "ping");
                assert!(args.is_empty());
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn directive_with_args() {
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.custom foo bar\n");
        match &doc.lines[0] {
            Line::Directive { name, args } => {
                assert_eq!(name, "kerai.custom");
                assert_eq!(args, &["foo", "bar"]);
            }
            other => panic!("expected Directive, got {other:?}"),
        }
    }

    #[test]
    fn quoted_args_in_call() {
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.prefix\npostgres.global.connection \"postgres://localhost/kerai\"");
        match &doc.lines[1] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "postgres.global.connection");
                assert_eq!(args, &[Expr::Atom("postgres://localhost/kerai".into())]);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn definition_with_multiple_words() {
        let mut parser = Parser::new();
        let doc = parser.parse("alias: some target value\n");
        match &doc.lines[0] {
            Line::Definition { name, target, .. } => {
                assert_eq!(name, "alias");
                assert_eq!(target, "some target value");
            }
            other => panic!("expected Definition, got {other:?}"),
        }
    }

    #[test]
    fn default_notation_tracks_final_state() {
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.infix\n");
        assert_eq!(doc.default_notation, Notation::Infix);
    }

    // --- New paren and expr tests ---

    #[test]
    fn infix_precedence() {
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.infix\n1 + 2 * 3\n");
        match &doc.lines[1] {
            Line::Call {
                function, args, ..
            } => {
                // + is function, args are [1, *(2,3)]
                assert_eq!(function, "+");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], Expr::Atom("1".into()));
                assert_eq!(
                    args[1],
                    Expr::Apply {
                        function: "*".into(),
                        args: vec![Expr::Atom("2".into()), Expr::Atom("3".into())],
                    }
                );
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn infix_parens_override() {
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.infix\n(1 + 2) * 3\n");
        match &doc.lines[1] {
            Line::Call {
                function, args, ..
            } => {
                assert_eq!(function, "*");
                assert_eq!(
                    args[0],
                    Expr::Apply {
                        function: "+".into(),
                        args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
                    }
                );
                assert_eq!(args[1], Expr::Atom("3".into()));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn prefix_with_paren_group() {
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.prefix\nadd (mul 2 3) 4\n");
        match &doc.lines[1] {
            Line::Call {
                function,
                args,
                notation,
            } => {
                assert_eq!(function, "add");
                assert_eq!(*notation, Notation::Prefix);
                assert_eq!(
                    args[0],
                    Expr::Apply {
                        function: "mul".into(),
                        args: vec![Expr::Atom("2".into()), Expr::Atom("3".into())],
                    }
                );
                assert_eq!(args[1], Expr::Atom("4".into()));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn scoped_notation_in_parens() {
        let mut parser = Parser::new();
        // Outer is prefix, inside parens we switch to postfix
        let doc = parser.parse("kerai.prefix\nadd (kerai.postfix 1 2 +) 4\n");
        match &doc.lines[1] {
            Line::Call {
                function,
                args,
                notation,
            } => {
                assert_eq!(function, "add");
                assert_eq!(*notation, Notation::Prefix);
                // Inner group parsed in postfix: 1 2 + → +(1, 2)
                assert_eq!(
                    args[0],
                    Expr::Apply {
                        function: "+".into(),
                        args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
                    }
                );
                assert_eq!(args[1], Expr::Atom("4".into()));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn alias_resolved_directive_in_parens() {
        let mut parser = Parser::new();
        // Define alias, set prefix mode, then use alias in a paren group
        let doc = parser.parse("k: kerai\nkerai.prefix\nadd (k.postfix 1 2 +) 4\n");
        assert_eq!(doc.lines.len(), 3);
        match &doc.lines[2] {
            Line::Call {
                function, args, ..
            } => {
                assert_eq!(function, "add");
                assert_eq!(
                    args[0],
                    Expr::Apply {
                        function: "+".into(),
                        args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
                    }
                );
                assert_eq!(args[1], Expr::Atom("4".into()));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn notation_reverts_after_paren_group() {
        let mut parser = Parser::new();
        // Set prefix. Paren group uses infix. After group, back to prefix.
        let doc = parser.parse("kerai.prefix\n(kerai.infix 1 + 2)\nadd 3 4\n");
        // Line 1: paren group parsed as infix
        match &doc.lines[1] {
            Line::Call {
                function,
                args,
                notation,
            } => {
                assert_eq!(function, "+");
                assert_eq!(*notation, Notation::Prefix);
                assert_eq!(args[0], Expr::Atom("1".into()));
                assert_eq!(args[1], Expr::Atom("2".into()));
            }
            other => panic!("expected Call for paren group, got {other:?}"),
        }
        // Line 2: still prefix (reverted after paren group)
        match &doc.lines[2] {
            Line::Call {
                function,
                notation,
                ..
            } => {
                assert_eq!(function, "add");
                assert_eq!(*notation, Notation::Prefix);
            }
            other => panic!("expected prefix Call, got {other:?}"),
        }
    }

    #[test]
    fn alias_directive_resolves_for_top_level() {
        let mut parser = Parser::new();
        // Alias makes k.infix resolve to kerai.infix
        let doc = parser.parse("k: kerai\nk.infix\na b c\n");
        assert!(matches!(&doc.lines[1], Line::Directive { name, .. } if name == "k.infix"));
        // Line 2 should be infix
        match &doc.lines[2] {
            Line::Call { notation, .. } => {
                assert_eq!(*notation, Notation::Infix);
            }
            other => panic!("expected infix Call, got {other:?}"),
        }
    }

    #[test]
    fn postfix_with_paren_group() {
        let mut parser = Parser::new();
        let doc = parser.parse("(1 2 mul) 4 add\n");
        match &doc.lines[0] {
            Line::Call {
                function, args, ..
            } => {
                assert_eq!(function, "add");
                assert_eq!(
                    args[0],
                    Expr::Apply {
                        function: "mul".into(),
                        args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
                    }
                );
                assert_eq!(args[1], Expr::Atom("4".into()));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn extract_paren_group_basic() {
        let tokens = tokenize("(a b c)");
        let (inner, end) = extract_paren_group(&tokens, 0).unwrap();
        assert_eq!(inner.len(), 3);
        assert_eq!(inner[0].value, "a");
        assert_eq!(inner[1].value, "b");
        assert_eq!(inner[2].value, "c");
        assert_eq!(end, 4); // index of ')'
    }

    #[test]
    fn extract_nested_paren_group() {
        let tokens = tokenize("(a (b c) d)");
        let (inner, end) = extract_paren_group(&tokens, 0).unwrap();
        assert_eq!(inner.len(), 6); // a ( b c ) d
        assert_eq!(end, 7);
    }

    // --- Postfix stack-based tests ---

    #[test]
    fn postfix_stack_binary_op() {
        // 1 2 + → +(1, 2)
        let mut parser = Parser::new();
        let doc = parser.parse("1 2 +\n");
        match &doc.lines[0] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "+");
                assert_eq!(args, &[Expr::Atom("1".into()), Expr::Atom("2".into())]);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn postfix_stack_chained_ops() {
        // 1 2 3 * + → +(1, *(2, 3))
        let mut parser = Parser::new();
        let doc = parser.parse("1 2 3 * +\n");
        match &doc.lines[0] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "+");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], Expr::Atom("1".into()));
                assert_eq!(
                    args[1],
                    Expr::Apply {
                        function: "*".into(),
                        args: vec![Expr::Atom("2".into()), Expr::Atom("3".into())],
                    }
                );
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn postfix_with_list() {
        // 1 [2 3 4 5] + → +(1, List([2, 3, 4, 5]))
        let mut parser = Parser::new();
        let doc = parser.parse("1 [2 3 4 5] +\n");
        match &doc.lines[0] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "+");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], Expr::Atom("1".into()));
                assert_eq!(
                    args[1],
                    Expr::List(vec![
                        Expr::Atom("2".into()),
                        Expr::Atom("3".into()),
                        Expr::Atom("4".into()),
                        Expr::Atom("5".into()),
                    ])
                );
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn postfix_flat_fallback() {
        // a b c (no known ops) → c(a, b) — flat fallback
        let mut parser = Parser::new();
        let doc = parser.parse("a b c\n");
        match &doc.lines[0] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "c");
                assert_eq!(args, &[Expr::Atom("a".into()), Expr::Atom("b".into())]);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    // --- Prefix recursive tests ---

    #[test]
    fn prefix_binary_op() {
        // + 1 2 → +(1, 2)
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.prefix\n+ 1 2\n");
        match &doc.lines[1] {
            Line::Call { function, args, notation, .. } => {
                assert_eq!(function, "+");
                assert_eq!(*notation, Notation::Prefix);
                assert_eq!(args, &[Expr::Atom("1".into()), Expr::Atom("2".into())]);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn prefix_nested_ops() {
        // + 1 * 2 3 → +(1, *(2, 3))
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.prefix\n+ 1 * 2 3\n");
        match &doc.lines[1] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "+");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], Expr::Atom("1".into()));
                assert_eq!(
                    args[1],
                    Expr::Apply {
                        function: "*".into(),
                        args: vec![Expr::Atom("2".into()), Expr::Atom("3".into())],
                    }
                );
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn prefix_with_list() {
        // + 1 [2 3 4 5] → +(1, List([2, 3, 4, 5]))
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.prefix\n+ 1 [2 3 4 5]\n");
        match &doc.lines[1] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "+");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], Expr::Atom("1".into()));
                assert_eq!(
                    args[1],
                    Expr::List(vec![
                        Expr::Atom("2".into()),
                        Expr::Atom("3".into()),
                        Expr::Atom("4".into()),
                        Expr::Atom("5".into()),
                    ])
                );
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn prefix_flat_fallback() {
        // foo bar baz (no known ops) → foo(bar, baz) — flat fallback
        let mut parser = Parser::new();
        let doc = parser.parse("kerai.prefix\nfoo bar baz\n");
        match &doc.lines[1] {
            Line::Call { function, args, notation, .. } => {
                assert_eq!(function, "foo");
                assert_eq!(*notation, Notation::Prefix);
                assert_eq!(args, &[Expr::Atom("bar".into()), Expr::Atom("baz".into())]);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    // --- Bracket/list tests in parser ---

    #[test]
    fn bracket_as_quotation_no_eval() {
        // [1 2 +] → no evaluation inside brackets
        let mut parser = Parser::new();
        let doc = parser.parse("[1 2 +]\n");
        // The whole line is a list — becomes list(List(...))
        match &doc.lines[0] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "list");
                assert_eq!(args.len(), 1);
                assert_eq!(
                    args[0],
                    Expr::List(vec![
                        Expr::Atom("1".into()),
                        Expr::Atom("2".into()),
                        Expr::Atom("+".into()),
                    ])
                );
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn nested_bracket_list_parser() {
        // [1 [2 3] 4] → List([1, List([2, 3]), 4])
        let mut parser = Parser::new();
        let doc = parser.parse("[1 [2 3] 4]\n");
        match &doc.lines[0] {
            Line::Call { function, args, .. } => {
                assert_eq!(function, "list");
                assert_eq!(
                    args[0],
                    Expr::List(vec![
                        Expr::Atom("1".into()),
                        Expr::List(vec![
                            Expr::Atom("2".into()),
                            Expr::Atom("3".into()),
                        ]),
                        Expr::Atom("4".into()),
                    ])
                );
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn extract_bracket_group_basic() {
        let tokens = tokenize("[a b c]");
        let (inner, end) = extract_bracket_group(&tokens, 0).unwrap();
        assert_eq!(inner.len(), 3);
        assert_eq!(inner[0].value, "a");
        assert_eq!(inner[1].value, "b");
        assert_eq!(inner[2].value, "c");
        assert_eq!(end, 4);
    }

    #[test]
    fn extract_nested_bracket_group() {
        let tokens = tokenize("[a [b c] d]");
        let (inner, end) = extract_bracket_group(&tokens, 0).unwrap();
        assert_eq!(inner.len(), 6); // a [ b c ] d
        assert_eq!(end, 7);
    }
}
