/// A notation-agnostic expression tree.
///
/// Parsed from any notation mode (prefix, infix, postfix) into a uniform
/// representation. `Atom` is a leaf value; `Apply` is function application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Leaf value: literal, identifier, or any single token.
    Atom(String),

    /// Function application: `function(args...)`.
    /// Notation-agnostic — always stores function name + ordered args
    /// regardless of how it was originally written.
    Apply { function: String, args: Vec<Expr> },
}

/// A token in postfix (RPN) form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostfixToken {
    Operand(String),
    Operator(String),
}

impl Expr {
    /// Convert this expression tree to a flat postfix (RPN) token sequence.
    ///
    /// - `Atom("x")` → `[Operand("x")]`
    /// - `Apply { function: "+", args: [a, b] }` → `[...a_postfix, ...b_postfix, Operator("+")]`
    pub fn to_postfix(&self) -> Vec<PostfixToken> {
        match self {
            Expr::Atom(s) => vec![PostfixToken::Operand(s.clone())],
            Expr::Apply { function, args } => {
                let mut out = Vec::new();
                for arg in args {
                    out.extend(arg.to_postfix());
                }
                out.push(PostfixToken::Operator(function.clone()));
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atom_to_postfix() {
        let e = Expr::Atom("42".into());
        assert_eq!(e.to_postfix(), vec![PostfixToken::Operand("42".into())]);
    }

    #[test]
    fn flat_apply_to_postfix() {
        // add(1, 2) → 1 2 add
        let e = Expr::Apply {
            function: "add".into(),
            args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
        };
        assert_eq!(
            e.to_postfix(),
            vec![
                PostfixToken::Operand("1".into()),
                PostfixToken::Operand("2".into()),
                PostfixToken::Operator("add".into()),
            ]
        );
    }

    #[test]
    fn nested_apply_to_postfix() {
        // +(1, *(2, 3)) → 1 2 3 * +
        let e = Expr::Apply {
            function: "+".into(),
            args: vec![
                Expr::Atom("1".into()),
                Expr::Apply {
                    function: "*".into(),
                    args: vec![Expr::Atom("2".into()), Expr::Atom("3".into())],
                },
            ],
        };
        assert_eq!(
            e.to_postfix(),
            vec![
                PostfixToken::Operand("1".into()),
                PostfixToken::Operand("2".into()),
                PostfixToken::Operand("3".into()),
                PostfixToken::Operator("*".into()),
                PostfixToken::Operator("+".into()),
            ]
        );
    }

    #[test]
    fn deeply_nested_to_postfix() {
        // +(*(a, b), -(c, d)) → a b * c d - +
        let e = Expr::Apply {
            function: "+".into(),
            args: vec![
                Expr::Apply {
                    function: "*".into(),
                    args: vec![Expr::Atom("a".into()), Expr::Atom("b".into())],
                },
                Expr::Apply {
                    function: "-".into(),
                    args: vec![Expr::Atom("c".into()), Expr::Atom("d".into())],
                },
            ],
        };
        assert_eq!(
            e.to_postfix(),
            vec![
                PostfixToken::Operand("a".into()),
                PostfixToken::Operand("b".into()),
                PostfixToken::Operator("*".into()),
                PostfixToken::Operand("c".into()),
                PostfixToken::Operand("d".into()),
                PostfixToken::Operator("-".into()),
                PostfixToken::Operator("+".into()),
            ]
        );
    }
}
