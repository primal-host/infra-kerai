use super::expr::Expr;
use super::token::{Token, TokenKind};

/// Binding power pair for an infix operator (left, right).
/// Left-associative: right = left + 1.
fn infix_binding_power(op: &str) -> (u8, u8) {
    match op {
        "+" | "-" => (10, 11),
        "*" | "/" | "%" => (20, 21),
        _ => (5, 6), // unknown operators get low precedence
    }
}

/// Returns true if the token can be an infix operator.
/// In kerai infix mode, any non-quoted word can be an operator — unknown
/// identifiers get low precedence (5, 6). Parens and quoted tokens are never operators.
fn is_operator(token: &Token) -> bool {
    !token.quoted && token.kind == TokenKind::Word
}

/// Pratt parser for infix expressions with operator precedence.
///
/// Handles:
/// - Atoms (identifiers, literals)
/// - Binary infix operators with precedence
/// - Parenthesized sub-expressions
pub struct PrattParser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> PrattParser<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        PrattParser { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// Parse an expression with minimum binding power.
    pub fn parse_expr(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.parse_atom()?;

        loop {
            let op = match self.peek() {
                Some(tok) if is_operator(tok) => tok.value.clone(),
                _ => break,
            };

            let (l_bp, r_bp) = infix_binding_power(&op);
            if l_bp < min_bp {
                break;
            }

            self.advance(); // consume operator

            match self.parse_expr(r_bp) {
                Some(rhs) => {
                    lhs = Expr::Apply {
                        function: op,
                        args: vec![lhs, rhs],
                    };
                }
                None => {
                    // No right operand — treat as unary application: op(lhs)
                    lhs = Expr::Apply {
                        function: op,
                        args: vec![lhs],
                    };
                    break;
                }
            }
        }

        Some(lhs)
    }

    /// Parse an atom: literal/identifier or parenthesized sub-expression.
    fn parse_atom(&mut self) -> Option<Expr> {
        let tok = self.peek()?;

        match tok.kind {
            TokenKind::LParen => {
                self.advance(); // consume '('
                let inner = self.parse_expr(0)?;
                // Expect closing paren
                if let Some(tok) = self.peek() {
                    if tok.kind == TokenKind::RParen {
                        self.advance(); // consume ')'
                    }
                }
                Some(inner)
            }
            TokenKind::LBracket => {
                self.advance(); // consume '['
                let mut elements = Vec::new();
                loop {
                    match self.peek() {
                        None => break,
                        Some(tok) if tok.kind == TokenKind::RBracket => {
                            self.advance(); // consume ']'
                            break;
                        }
                        Some(tok) if tok.kind == TokenKind::LBracket => {
                            // Nested list — recurse via parse_atom
                            if let Some(inner) = self.parse_atom() {
                                elements.push(inner);
                            }
                        }
                        Some(tok) if tok.kind == TokenKind::LParen => {
                            // Paren group inside brackets — parse as sub-expression
                            if let Some(inner) = self.parse_atom() {
                                elements.push(inner);
                            }
                        }
                        Some(_) => {
                            // Word — everything is quoted (no operator evaluation)
                            let val = self.peek().unwrap().value.clone();
                            self.advance();
                            elements.push(Expr::Atom(val));
                        }
                    }
                }
                Some(Expr::List(elements))
            }
            TokenKind::Word => {
                let val = tok.value.clone();
                self.advance();
                Some(Expr::Atom(val))
            }
            TokenKind::RParen | TokenKind::RBracket => None, // unexpected — let caller handle
        }
    }

    /// Returns true if all tokens have been consumed.
    pub fn is_done(&self) -> bool {
        self.pos >= self.tokens.len()
    }
}

/// Parse a token slice as an infix expression using Pratt parsing.
/// Returns the parsed expression, or None if tokens are empty.
pub fn parse_infix(tokens: &[Token]) -> Option<Expr> {
    if tokens.is_empty() {
        return None;
    }
    let mut parser = PrattParser::new(tokens);
    parser.parse_expr(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::token::tokenize;

    #[test]
    fn single_atom() {
        let tokens = tokenize("42");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(expr, Expr::Atom("42".into()));
    }

    #[test]
    fn simple_addition() {
        let tokens = tokenize("1 + 2");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "+".into(),
                args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
            }
        );
    }

    #[test]
    fn precedence_mul_over_add() {
        // 1 + 2 * 3 → +(1, *(2, 3))
        let tokens = tokenize("1 + 2 * 3");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "+".into(),
                args: vec![
                    Expr::Atom("1".into()),
                    Expr::Apply {
                        function: "*".into(),
                        args: vec![Expr::Atom("2".into()), Expr::Atom("3".into())],
                    },
                ],
            }
        );
    }

    #[test]
    fn parens_override_precedence() {
        // (1 + 2) * 3 → *(+(1, 2), 3)
        let tokens = tokenize("(1 + 2) * 3");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "*".into(),
                args: vec![
                    Expr::Apply {
                        function: "+".into(),
                        args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
                    },
                    Expr::Atom("3".into()),
                ],
            }
        );
    }

    #[test]
    fn left_associativity() {
        // 1 - 2 - 3 → -(-(1, 2), 3)
        let tokens = tokenize("1 - 2 - 3");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "-".into(),
                args: vec![
                    Expr::Apply {
                        function: "-".into(),
                        args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
                    },
                    Expr::Atom("3".into()),
                ],
            }
        );
    }

    #[test]
    fn nested_parens() {
        // ((1 + 2)) → +(1, 2)
        let tokens = tokenize("((1 + 2))");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "+".into(),
                args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
            }
        );
    }

    #[test]
    fn complex_expression() {
        // a + b * c - d → -(+(a, *(b, c)), d)
        let tokens = tokenize("a + b * c - d");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "-".into(),
                args: vec![
                    Expr::Apply {
                        function: "+".into(),
                        args: vec![
                            Expr::Atom("a".into()),
                            Expr::Apply {
                                function: "*".into(),
                                args: vec![Expr::Atom("b".into()), Expr::Atom("c".into())],
                            },
                        ],
                    },
                    Expr::Atom("d".into()),
                ],
            }
        );
    }

    #[test]
    fn empty_tokens() {
        let tokens = tokenize("");
        assert_eq!(parse_infix(&tokens), None);
    }

    #[test]
    fn word_as_operator() {
        // a b c → b(a, c) — word tokens work as infix operators
        let tokens = tokenize("a b c");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "b".into(),
                args: vec![Expr::Atom("a".into()), Expr::Atom("c".into())],
            }
        );
    }

    #[test]
    fn bracket_list_literal() {
        let tokens = tokenize("[1 2 3]");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::List(vec![
                Expr::Atom("1".into()),
                Expr::Atom("2".into()),
                Expr::Atom("3".into()),
            ])
        );
    }

    #[test]
    fn infix_with_list_operand() {
        // 1 + [2 3 4] → +(1, List([2, 3, 4]))
        let tokens = tokenize("1 + [2 3 4]");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "+".into(),
                args: vec![
                    Expr::Atom("1".into()),
                    Expr::List(vec![
                        Expr::Atom("2".into()),
                        Expr::Atom("3".into()),
                        Expr::Atom("4".into()),
                    ]),
                ],
            }
        );
    }

    #[test]
    fn bracket_as_quotation() {
        // [1 2 +] — no evaluation inside brackets
        let tokens = tokenize("[1 2 +]");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::List(vec![
                Expr::Atom("1".into()),
                Expr::Atom("2".into()),
                Expr::Atom("+".into()),
            ])
        );
    }

    #[test]
    fn nested_bracket_list() {
        // [1 [2 3] 4] → List([1, List([2, 3]), 4])
        let tokens = tokenize("[1 [2 3] 4]");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
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

    #[test]
    fn two_token_unary() {
        // c d → d(c) — missing right operand treated as unary
        let tokens = tokenize("c d");
        let expr = parse_infix(&tokens).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "d".into(),
                args: vec![Expr::Atom("c".into())],
            }
        );
    }
}
