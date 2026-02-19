use std::collections::HashMap;

use super::ast::{Document, Line, Notation};
use super::expr::Expr;
use super::pratt;
use super::token::{tokenize, Token, TokenKind};

/// Parser with a notation mode stack and alias tracking for `.kerai` files.
pub struct Parser {
    notation_stack: Vec<Notation>,
    aliases: HashMap<String, String>,
}

impl Parser {
    pub fn new() -> Self {
        Parser {
            notation_stack: vec![Notation::Prefix],
            aliases: HashMap::new(),
        }
    }

    /// Current notation mode (top of stack).
    fn notation(&self) -> Notation {
        *self.notation_stack.last().unwrap_or(&Notation::Prefix)
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

        // Definition line: `:name target`
        if let Some(rest) = trimmed.strip_prefix(':') {
            let tokens = tokenize(rest);
            if tokens.len() >= 2 {
                return Line::Definition {
                    name: tokens[0].value.clone(),
                    target: tokens[1..]
                        .iter()
                        .map(|t| t.value.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                    notation: self.notation(),
                };
            }
            // Malformed definition — treat as comment
            return Line::Comment {
                text: raw.to_string(),
            };
        }

        let tokens = tokenize(trimmed);
        if tokens.is_empty() {
            return Line::Empty;
        }

        // Type annotation: first token ends with `:`
        if tokens[0].value.ends_with(':') && !tokens[0].quoted {
            let name = tokens[0].value.trim_end_matches(':').to_string();
            let type_expr = tokens[1..]
                .iter()
                .map(|t| t.value.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            return Line::TypeAnnotation { name, type_expr };
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

        // Check if there are any parens in the token stream
        let has_parens = tokens.iter().any(|t| t.kind == TokenKind::LParen || t.kind == TokenKind::RParen);

        // Function call — dispatch based on notation mode
        let notation = self.notation();

        if has_parens || notation == Notation::Infix {
            // Use expression-based parsing
            return self.parse_expr_line(&tokens, notation);
        }

        // Flat path (no parens, prefix/postfix) — unchanged backward compat
        let values: Vec<String> = tokens.into_iter().map(|t| t.value).collect();

        if values.len() == 1 {
            return Line::Call {
                function: values[0].clone(),
                args: vec![],
                notation,
            };
        }

        let (function, args) = match notation {
            Notation::Prefix => {
                let function = values[0].clone();
                let args: Vec<Expr> = values[1..].iter().map(|v| Expr::Atom(v.clone())).collect();
                (function, args)
            }
            Notation::Postfix => {
                let function = values.last().unwrap().clone();
                let args: Vec<Expr> = values[..values.len() - 1]
                    .iter()
                    .map(|v| Expr::Atom(v.clone()))
                    .collect();
                (function, args)
            }
            Notation::Infix => unreachable!("infix handled above"),
        };

        Line::Call {
            function,
            args,
            notation,
        }
    }

    /// Parse a line with expression-aware parsing (handles parens and infix).
    fn parse_expr_line(&mut self, tokens: &[Token], notation: Notation) -> Line {
        match notation {
            Notation::Infix => self.parse_infix_line(tokens),
            Notation::Prefix => self.parse_prefix_line_with_parens(tokens),
            Notation::Postfix => self.parse_postfix_line_with_parens(tokens),
        }
    }

    /// Parse an infix line using the Pratt parser.
    fn parse_infix_line(&mut self, tokens: &[Token]) -> Line {
        match pratt::parse_infix(tokens) {
            Some(Expr::Atom(s)) => Line::Call {
                function: s,
                args: vec![],
                notation: Notation::Infix,
            },
            Some(Expr::Apply { function, args }) => Line::Call {
                function,
                args,
                notation: Notation::Infix,
            },
            None => Line::Empty,
        }
    }

    /// Parse a prefix line that contains parenthesized sub-expressions.
    fn parse_prefix_line_with_parens(&mut self, tokens: &[Token]) -> Line {
        // Parse all tokens as expressions (handling paren groups)
        let exprs = self.parse_arg_list(tokens, Notation::Prefix);

        if exprs.is_empty() {
            return Line::Empty;
        }

        // Single expression — unwrap it as the line's call
        if exprs.len() == 1 {
            return match exprs.into_iter().next().unwrap() {
                Expr::Atom(s) => Line::Call {
                    function: s,
                    args: vec![],
                    notation: Notation::Prefix,
                },
                Expr::Apply { function, args } => Line::Call {
                    function,
                    args,
                    notation: Notation::Prefix,
                },
            };
        }

        // Multiple expressions: first is function, rest are args
        let mut iter = exprs.into_iter();
        let first = iter.next().unwrap();
        match first {
            Expr::Atom(function) => {
                let args: Vec<Expr> = iter.collect();
                Line::Call {
                    function,
                    args,
                    notation: Notation::Prefix,
                }
            }
            Expr::Apply { function, mut args } => {
                // First is already an Apply — extend with remaining
                args.extend(iter);
                Line::Call {
                    function,
                    args,
                    notation: Notation::Prefix,
                }
            }
        }
    }

    /// Parse a postfix line that contains parenthesized sub-expressions.
    fn parse_postfix_line_with_parens(&mut self, tokens: &[Token]) -> Line {
        // Collect all expressions from the token stream
        let exprs = self.parse_arg_list(tokens, Notation::Postfix);

        if exprs.is_empty() {
            return Line::Empty;
        }

        if exprs.len() == 1 {
            // Single expression — check if it's an Apply or Atom
            return match exprs.into_iter().next().unwrap() {
                Expr::Atom(s) => Line::Call {
                    function: s,
                    args: vec![],
                    notation: Notation::Postfix,
                },
                Expr::Apply { function, args } => Line::Call {
                    function,
                    args,
                    notation: Notation::Postfix,
                },
            };
        }

        // Stack-based postfix: last expr is operator, rest are operands
        // For flat tokens: last is function, rest are args
        let mut exprs = exprs;
        let last = exprs.pop().unwrap();
        match last {
            Expr::Atom(function) => Line::Call {
                function,
                args: exprs,
                notation: Notation::Postfix,
            },
            Expr::Apply { function, args: inner_args } => {
                // The last expression is itself an Apply — it becomes the entire call
                // with the preceding expressions prepended as additional args
                let mut all_args = exprs;
                all_args.extend(inner_args);
                Line::Call {
                    function,
                    args: all_args,
                    notation: Notation::Postfix,
                }
            }
        }
    }

    /// Parse a token slice into a list of `Expr` values, handling paren groups.
    fn parse_arg_list(&mut self, tokens: &[Token], notation: Notation) -> Vec<Expr> {
        let mut args = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            match tokens[i].kind {
                TokenKind::LParen => {
                    // Find matching close paren
                    if let Some((inner, end)) = extract_paren_group(tokens, i) {
                        let expr = self.parse_paren_group(inner, notation);
                        args.push(expr);
                        i = end + 1;
                    } else {
                        // Unmatched paren — treat as atom
                        args.push(Expr::Atom(tokens[i].value.clone()));
                        i += 1;
                    }
                }
                TokenKind::RParen => {
                    // Unexpected close paren — skip
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
            Notation::Prefix => {
                let args = self.parse_arg_list(tokens, notation);
                if args.is_empty() {
                    Expr::Atom(String::new())
                } else if args.len() == 1 {
                    args.into_iter().next().unwrap()
                } else {
                    // First is function, rest are args
                    let mut iter = args.into_iter();
                    let first = iter.next().unwrap();
                    let function = match first {
                        Expr::Atom(s) => s,
                        _ => return first, // single nested expr
                    };
                    let rest: Vec<Expr> = iter.collect();
                    Expr::Apply {
                        function,
                        args: rest,
                    }
                }
            }
            Notation::Postfix => {
                let args = self.parse_arg_list(tokens, notation);
                if args.is_empty() {
                    Expr::Atom(String::new())
                } else if args.len() == 1 {
                    args.into_iter().next().unwrap()
                } else {
                    // Stack-based: last is operator, rest are operands
                    let mut args = args;
                    let last = args.pop().unwrap();
                    match last {
                        Expr::Atom(function) => Expr::Apply {
                            function,
                            args,
                        },
                        apply => {
                            // Last is already an Apply — return it
                            // (preceding operands become additional context)
                            args.push(apply);
                            if args.len() == 1 {
                                args.into_iter().next().unwrap()
                            } else {
                                // Multiple items with no clear operator — wrap last
                                let last = args.pop().unwrap();
                                match last {
                                    Expr::Apply { function, args: inner } => {
                                        let mut all = args;
                                        all.extend(inner);
                                        Expr::Apply { function, args: all }
                                    }
                                    other => other,
                                }
                            }
                        }
                    }
                }
            }
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
        let doc = parser.parse(":pg postgres\n");
        assert_eq!(doc.lines.len(), 1);
        match &doc.lines[0] {
            Line::Definition {
                name,
                target,
                notation,
            } => {
                assert_eq!(name, "pg");
                assert_eq!(target, "postgres");
                assert_eq!(*notation, Notation::Prefix);
            }
            other => panic!("expected Definition, got {other:?}"),
        }
    }

    #[test]
    fn type_annotation() {
        let mut parser = Parser::new();
        let doc = parser.parse("name: String\n");
        assert_eq!(doc.lines.len(), 1);
        match &doc.lines[0] {
            Line::TypeAnnotation { name, type_expr } => {
                assert_eq!(name, "name");
                assert_eq!(type_expr, "String");
            }
            other => panic!("expected TypeAnnotation, got {other:?}"),
        }
    }

    #[test]
    fn prefix_call() {
        let mut parser = Parser::new();
        let doc = parser.parse("postgres.global.connection localhost\n");
        assert_eq!(doc.lines.len(), 1);
        match &doc.lines[0] {
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
        // Line 0: prefix call a(b)
        assert!(
            matches!(&doc.lines[0], Line::Call { function, notation, .. } if function == "a" && *notation == Notation::Prefix)
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
        let doc = parser.parse(r#"postgres.global.connection "postgres://localhost/kerai""#);
        match &doc.lines[0] {
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
        let doc = parser.parse(":alias some target value\n");
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
        let doc = parser.parse("add (mul 2 3) 4\n");
        match &doc.lines[0] {
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
        // Default is prefix, but inside parens we switch to postfix
        let doc = parser.parse("add (kerai.postfix 1 2 +) 4\n");
        match &doc.lines[0] {
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
        // Define alias, then use it in a paren group
        let doc = parser.parse(":k kerai\nadd (k.postfix 1 2 +) 4\n");
        assert_eq!(doc.lines.len(), 2);
        match &doc.lines[1] {
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
        // Default prefix. Paren group uses infix. After group, back to prefix.
        let doc = parser.parse("(kerai.infix 1 + 2)\nadd 3 4\n");
        // Line 0: paren group parsed as infix
        // It's a top-level paren group, so the whole line becomes the expression
        match &doc.lines[0] {
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
        // Line 1: still prefix
        match &doc.lines[1] {
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
        let doc = parser.parse(":k kerai\nk.infix\na b c\n");
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
        let doc = parser.parse("kerai.postfix\n(1 2 mul) 4 add\n");
        match &doc.lines[1] {
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
}
