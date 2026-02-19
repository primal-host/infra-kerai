pub mod ast;
pub mod expr;
mod parser;
mod pratt;
pub mod token;

use std::fs;
use std::path::Path;

pub use ast::{Document, Line, Notation};
pub use expr::{Expr, PostfixToken};

/// Parse kerai language source text into a Document.
pub fn parse(source: &str) -> Document {
    let mut parser = parser::Parser::new();
    parser.parse(source)
}

/// Parse a `.kerai` file into a Document.
pub fn parse_file(path: &Path) -> Result<Document, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    Ok(parse(&content))
}

/// Parse a single expression from source text under a given notation mode.
pub fn parse_expr(source: &str, notation: Notation) -> Option<Expr> {
    let tokens = token::tokenize(source);
    if tokens.is_empty() {
        return None;
    }
    match notation {
        Notation::Infix => pratt::parse_infix(&tokens),
        Notation::Prefix => {
            let mut parser = parser::Parser::new();
            let doc = parser.parse(source);
            match doc.lines.into_iter().next()? {
                Line::Call { function, args, .. } => {
                    if args.is_empty() {
                        Some(Expr::Atom(function))
                    } else if function == "list" && args.len() == 1 && matches!(&args[0], Expr::List(_)) {
                        Some(args.into_iter().next().unwrap())
                    } else {
                        Some(Expr::Apply { function, args })
                    }
                }
                _ => None,
            }
        }
        Notation::Postfix => {
            let mut p = parser::Parser::new();
            p.push_notation(Notation::Postfix);
            let doc = p.parse(source);
            match doc.lines.into_iter().next()? {
                Line::Call { function, args, .. } => {
                    if args.is_empty() {
                        Some(Expr::Atom(function))
                    } else if function == "list" && args.len() == 1 && matches!(&args[0], Expr::List(_)) {
                        Some(args.into_iter().next().unwrap())
                    } else {
                        Some(Expr::Apply { function, args })
                    }
                }
                _ => None,
            }
        }
    }
}

/// Extract definitions from a document as `(name, target)` pairs.
pub fn definitions(doc: &Document) -> Vec<(&str, &str)> {
    doc.lines
        .iter()
        .filter_map(|line| match line {
            Line::Definition { name, target, .. } => Some((name.as_str(), target.as_str())),
            _ => None,
        })
        .collect()
}

/// Extract function calls from a document as `(function, args)` pairs.
/// Only extracts `Atom` args as flat strings — nested `Apply` args are skipped.
pub fn calls(doc: &Document) -> Vec<(&str, Vec<&str>)> {
    doc.lines
        .iter()
        .filter_map(|line| match line {
            Line::Call { function, args, .. } => {
                let flat: Vec<&str> = args
                    .iter()
                    .filter_map(|e| match e {
                        Expr::Atom(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .collect();
                Some((function.as_str(), flat))
            }
            _ => None,
        })
        .collect()
}

/// Render an `Expr` back to source text.
fn render_expr(expr: &Expr) -> String {
    match expr {
        Expr::Atom(s) => {
            if s.contains(' ') {
                format!("\"{s}\"")
            } else {
                s.clone()
            }
        }
        Expr::Apply { function, args } => {
            // Render as parenthesized prefix form: (function arg1 arg2)
            let rendered_args: Vec<String> = args.iter().map(render_expr).collect();
            if rendered_args.is_empty() {
                format!("({function})")
            } else {
                format!("({function} {})", rendered_args.join(" "))
            }
        }
        Expr::List(elements) => {
            let rendered: Vec<String> = elements.iter().map(render_expr).collect();
            format!("[{}]", rendered.join(" "))
        }
    }
}

/// Render a single line back to source text.
pub fn render_line(line: &Line) -> String {
    match line {
        Line::Empty => String::new(),
        Line::Comment { text } => text.clone(),
        Line::Definition { name, target, .. } => format!("{name}: {target}"),
        Line::Call {
            function, args, ..
        } => {
            if args.is_empty() {
                function.clone()
            } else {
                let rendered: Vec<String> = args.iter().map(render_expr).collect();
                format!("{function} {}", rendered.join(" "))
            }
        }
        Line::Directive { name, args } => {
            if args.is_empty() {
                name.clone()
            } else {
                format!("{name} {}", args.join(" "))
            }
        }
    }
}

/// Render a document back to source text (for round-tripping).
pub fn render(doc: &Document) -> String {
    let lines: Vec<String> = doc.lines.iter().map(render_line).collect();
    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_aliases_file() {
        let source = "# common aliases for kerai libraries\npg: postgres\n";
        let doc = parse(source);
        assert_eq!(doc.lines.len(), 2);

        let defs = definitions(&doc);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0], ("pg", "postgres"));
    }

    #[test]
    fn parse_kerai_config_file() {
        let source = "\
# kerai-controlled configuration — do not hand-edit
# syntax: :name target (definition) | name arg (function call) | name: type (reserved)
postgres.global.connection postgres://localhost/kerai
";
        let doc = parse(source);
        assert_eq!(doc.lines.len(), 3);

        let cs = calls(&doc);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].0, "postgres.global.connection");
        assert_eq!(cs[0].1, vec!["postgres://localhost/kerai"]);
    }

    #[test]
    fn definitions_extracts_only_definitions() {
        let doc = parse("a: b\nfoo bar\nc: d\n# comment\n");
        let defs = definitions(&doc);
        assert_eq!(defs, vec![("a", "b"), ("c", "d")]);
    }

    #[test]
    fn calls_extracts_only_calls() {
        let doc = parse("a: b\nfoo bar baz\n# comment\nping\n");
        let cs = calls(&doc);
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0], ("foo", vec!["bar", "baz"]));
        assert_eq!(cs[1], ("ping", Vec::<&str>::new()));
    }

    #[test]
    fn round_trip_preserves_structure() {
        let source = "\
# comment line
pg: postgres

postgres.global.connection localhost
";
        let doc = parse(source);
        let rendered = render(&doc);
        assert_eq!(rendered, source);
    }

    #[test]
    fn empty_document() {
        let doc = parse("");
        assert!(doc.lines.is_empty());
        assert_eq!(doc.default_notation, Notation::Prefix);
    }

    #[test]
    fn notation_modes_all_work() {
        let source = "\
a b c
kerai.infix
a b c
kerai.postfix
a b c
";
        let doc = parse(source);
        let cs = calls(&doc);
        assert_eq!(cs.len(), 3);
        // prefix: a(b, c)
        assert_eq!(cs[0], ("a", vec!["b", "c"]));
        // infix: b(a, c) — Pratt parser, b is unknown operator
        assert_eq!(cs[1], ("b", vec!["a", "c"]));
        // postfix: c(a, b)
        assert_eq!(cs[2], ("c", vec!["a", "b"]));
    }

    #[test]
    fn parse_expr_infix() {
        let expr = parse_expr("1 + 2 * 3", Notation::Infix).unwrap();
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
    fn parse_expr_prefix() {
        let expr = parse_expr("add 1 2", Notation::Prefix).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "add".into(),
                args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
            }
        );
    }

    #[test]
    fn parse_expr_postfix() {
        let expr = parse_expr("1 2 add", Notation::Postfix).unwrap();
        assert_eq!(
            expr,
            Expr::Apply {
                function: "add".into(),
                args: vec![Expr::Atom("1".into()), Expr::Atom("2".into())],
            }
        );
    }

    #[test]
    fn render_expr_nested() {
        let line = Line::Call {
            function: "add".into(),
            args: vec![
                Expr::Apply {
                    function: "mul".into(),
                    args: vec![Expr::Atom("2".into()), Expr::Atom("3".into())],
                },
                Expr::Atom("4".into()),
            ],
            notation: Notation::Prefix,
        };
        let rendered = render_line(&line);
        assert_eq!(rendered, "add (mul 2 3) 4");
    }

    #[test]
    fn render_list_expr() {
        let line = Line::Call {
            function: "add".into(),
            args: vec![
                Expr::Atom("1".into()),
                Expr::List(vec![
                    Expr::Atom("2".into()),
                    Expr::Atom("3".into()),
                    Expr::Atom("4".into()),
                ]),
            ],
            notation: Notation::Prefix,
        };
        let rendered = render_line(&line);
        assert_eq!(rendered, "add 1 [2 3 4]");
    }

    #[test]
    fn render_nested_list() {
        let line = Line::Call {
            function: "list".into(),
            args: vec![Expr::List(vec![
                Expr::Atom("1".into()),
                Expr::List(vec![
                    Expr::Atom("2".into()),
                    Expr::Atom("3".into()),
                ]),
                Expr::Atom("4".into()),
            ])],
            notation: Notation::Prefix,
        };
        let rendered = render_line(&line);
        assert_eq!(rendered, "list [1 [2 3] 4]");
    }

    #[test]
    fn calls_skips_list_args() {
        // calls() should skip List args, same as Apply
        let doc = parse("add [1 2 3] 4\n");
        let cs = calls(&doc);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].0, "add");
        assert_eq!(cs[0].1, vec!["4"]);
    }

    #[test]
    fn calls_skips_nested_apply_args() {
        // calls() should only extract Atom args for backward compat
        let doc = parse("add (mul 2 3) 4\n");
        let cs = calls(&doc);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].0, "add");
        // mul(2,3) is Apply, skipped; 4 is Atom, kept
        assert_eq!(cs[0].1, vec!["4"]);
    }
}
