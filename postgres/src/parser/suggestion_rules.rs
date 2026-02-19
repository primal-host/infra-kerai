/// Suggestion rules — detect code patterns that merit advisory comments.
///
/// Each rule has:
/// - A unique ID (used to match `// kerai:` comments back to suggestions)
/// - A severity (info or warning)
/// - A category (idiom, naming, dead_code, attribute)
/// - A detection function that analyzes syn AST nodes

/// A suggestion finding from a rule.
#[derive(Debug, Clone)]
pub struct Finding {
    pub rule_id: &'static str,
    pub message: String,
    pub severity: &'static str,
    pub category: &'static str,
    /// Line number where the suggestion applies (1-based).
    pub line: i32,
    /// Node ID of the target this suggestion is about.
    pub target_node_id: String,
}

/// Run all suggestion rules against a parsed syn::File and its node metadata.
///
/// `nodes` contains (node_id, kind, name, span_start, content) tuples from the AST walk.
pub fn run_rules(file: &syn::File, nodes: &[NodeInfo]) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Run rules that analyze the syn AST directly
    check_fn_params(file, nodes, &mut findings);
    check_naming_conventions(nodes, &mut findings);
    check_missing_debug_derive(file, nodes, &mut findings);

    findings
}

/// Node information passed from the AST walker for rule analysis.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub id: String,
    pub kind: String,
    pub name: Option<String>,
    pub span_start: Option<i32>,
    pub content: Option<String>,
    pub source: Option<String>,
}

// ── Idiom Rules ─────────────────────────────────────────────────────────

/// Check function parameters for common idiom issues:
/// - &String → &str
/// - &Vec<T> → &[T]
fn check_fn_params(
    file: &syn::File,
    nodes: &[NodeInfo],
    findings: &mut Vec<Finding>,
) {
    use syn::visit::Visit;

    struct FnVisitor<'a> {
        nodes: &'a [NodeInfo],
        findings: &'a mut Vec<Finding>,
    }

    impl<'ast, 'a> Visit<'ast> for FnVisitor<'a> {
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let fn_name = item.sig.ident.to_string();
            let line = item.sig.ident.span().start().line as i32;

            // Find matching node
            let node = self.nodes.iter().find(|n| {
                n.kind == "fn" && n.name.as_deref() == Some(&fn_name) && n.span_start == Some(line)
            });

            if let Some(node) = node {
                for param in &item.sig.inputs {
                    if let syn::FnArg::Typed(pat_type) = param {
                        check_param_type(&pat_type.ty, &node.id, line, self.findings);
                    }
                }
            }

            syn::visit::visit_item_fn(self, item);
        }

        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            let fn_name = item.sig.ident.to_string();
            let line = item.sig.ident.span().start().line as i32;

            let node = self.nodes.iter().find(|n| {
                n.kind == "fn" && n.name.as_deref() == Some(&fn_name) && n.span_start == Some(line)
            });

            if let Some(node) = node {
                for param in &item.sig.inputs {
                    if let syn::FnArg::Typed(pat_type) = param {
                        check_param_type(&pat_type.ty, &node.id, line, self.findings);
                    }
                }
            }

            syn::visit::visit_impl_item_fn(self, item);
        }
    }

    let mut visitor = FnVisitor { nodes, findings };
    visitor.visit_file(file);
}

/// Check a parameter type for idiom issues.
fn check_param_type(
    ty: &syn::Type,
    target_node_id: &str,
    line: i32,
    findings: &mut Vec<Finding>,
) {
    if let syn::Type::Reference(type_ref) = ty {
        let inner = &*type_ref.elem;
        let inner_str = quote::quote!(#inner).to_string();

        // &String → &str
        if inner_str == "String" {
            findings.push(Finding {
                rule_id: "prefer_str_slice",
                message: "consider &str instead of &String".to_string(),
                severity: "info",
                category: "idiom",
                line,
                target_node_id: target_node_id.to_string(),
            });
        }

        // &Vec<T> → &[T]
        if inner_str.starts_with("Vec <") || inner_str.starts_with("Vec<") {
            findings.push(Finding {
                rule_id: "prefer_slice",
                message: "consider &[T] instead of &Vec<T>".to_string(),
                severity: "info",
                category: "idiom",
                line,
                target_node_id: target_node_id.to_string(),
            });
        }
    }
}

// ── Naming Rules ─────────────────────────────────────────────────────────

/// Check naming conventions for functions, variables, types, and constants.
fn check_naming_conventions(nodes: &[NodeInfo], findings: &mut Vec<Finding>) {
    for node in nodes {
        let name = match &node.name {
            Some(n) => n.as_str(),
            None => continue,
        };
        let line = match node.span_start {
            Some(l) => l,
            None => continue,
        };

        match node.kind.as_str() {
            "fn" => {
                if !is_snake_case(name) && !name.starts_with('_') {
                    let suggestion = to_snake_case(name);
                    findings.push(Finding {
                        rule_id: "non_snake_fn",
                        message: format!("function names should be snake_case: {}", suggestion),
                        severity: "warning",
                        category: "naming",
                        line,
                        target_node_id: node.id.clone(),
                    });
                }
            }
            "struct" | "enum" | "trait" | "union" | "type_alias" => {
                if !is_camel_case(name) && !name.starts_with('_') {
                    findings.push(Finding {
                        rule_id: "non_camel_type",
                        message: format!("type names should be CamelCase: {}", to_camel_case(name)),
                        severity: "warning",
                        category: "naming",
                        line,
                        target_node_id: node.id.clone(),
                    });
                }
            }
            "const" | "static" => {
                if !is_upper_snake_case(name)
                    && !name.starts_with('_')
                    // Skip common patterns like type aliases (lowercase statics are common in FFI)
                    && node.kind.as_str() == "const"
                {
                    findings.push(Finding {
                        rule_id: "non_upper_const",
                        message: format!(
                            "constants should be UPPER_SNAKE_CASE: {}",
                            name.to_uppercase()
                        ),
                        severity: "info",
                        category: "naming",
                        line,
                        target_node_id: node.id.clone(),
                    });
                }
            }
            _ => {}
        }
    }
}

/// Check for structs/enums without Debug derive.
fn check_missing_debug_derive(
    file: &syn::File,
    nodes: &[NodeInfo],
    findings: &mut Vec<Finding>,
) {
    use syn::visit::Visit;

    struct DeriveVisitor<'a> {
        nodes: &'a [NodeInfo],
        findings: &'a mut Vec<Finding>,
    }

    impl<'ast, 'a> Visit<'ast> for DeriveVisitor<'a> {
        fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
            check_debug_derive(&item.attrs, &item.ident, "struct", self.nodes, self.findings);
            syn::visit::visit_item_struct(self, item);
        }

        fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
            check_debug_derive(&item.attrs, &item.ident, "enum", self.nodes, self.findings);
            syn::visit::visit_item_enum(self, item);
        }
    }

    let mut visitor = DeriveVisitor { nodes, findings };
    visitor.visit_file(file);
}

fn check_debug_derive(
    attrs: &[syn::Attribute],
    ident: &syn::Ident,
    kind: &str,
    nodes: &[NodeInfo],
    findings: &mut Vec<Finding>,
) {
    let has_debug = attrs.iter().any(|attr| {
        if !attr.path().is_ident("derive") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("Debug") {
                found = true;
            }
            Ok(())
        });
        found
    });

    if !has_debug {
        let name = ident.to_string();
        let line = ident.span().start().line as i32;
        let node = nodes.iter().find(|n| {
            n.kind == kind && n.name.as_deref() == Some(&name) && n.span_start == Some(line)
        });

        if let Some(node) = node {
            findings.push(Finding {
                rule_id: "missing_derive_debug",
                message: "consider deriving Debug".to_string(),
                severity: "info",
                category: "attribute",
                line,
                target_node_id: node.id.clone(),
            });
        }
    }
}

// ── Naming Helpers ──────────────────────────────────────────────────────

fn is_snake_case(s: &str) -> bool {
    s.chars().all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_camel_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.chars().next().unwrap();
    first.is_uppercase() && !s.contains('_')
}

fn is_upper_snake_case(s: &str) -> bool {
    s.chars().all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_')
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
}

fn to_camel_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_snake_case() {
        assert!(is_snake_case("foo_bar"));
        assert!(is_snake_case("foo"));
        assert!(!is_snake_case("fooBar"));
        assert!(!is_snake_case("FooBar"));
    }

    #[test]
    fn test_is_camel_case() {
        assert!(is_camel_case("FooBar"));
        assert!(is_camel_case("Foo"));
        assert!(!is_camel_case("foo_bar"));
        assert!(!is_camel_case("FOO_BAR"));
    }

    #[test]
    fn test_is_upper_snake_case() {
        assert!(is_upper_snake_case("FOO_BAR"));
        assert!(is_upper_snake_case("FOO"));
        assert!(!is_upper_snake_case("foo_bar"));
        assert!(!is_upper_snake_case("FooBar"));
    }

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("FooBar"), "foo_bar");
        assert_eq!(to_snake_case("myFunc"), "my_func");
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("foo_bar"), "FooBar");
        assert_eq!(to_camel_case("my_struct"), "MyStruct");
    }

    #[test]
    fn test_check_param_type_string() {
        let source = "fn foo(s: &String) {}";
        let file = syn::parse_file(source).unwrap();
        let nodes = vec![NodeInfo {
            id: "test-id".into(),
            kind: "fn".into(),
            name: Some("foo".into()),
            span_start: Some(1),
            content: None,
            source: None,
        }];

        let findings = run_rules(&file, &nodes);
        assert!(findings.iter().any(|f| f.rule_id == "prefer_str_slice"));
    }

    #[test]
    fn test_check_param_type_vec() {
        let source = "fn foo(v: &Vec<i32>) {}";
        let file = syn::parse_file(source).unwrap();
        let nodes = vec![NodeInfo {
            id: "test-id".into(),
            kind: "fn".into(),
            name: Some("foo".into()),
            span_start: Some(1),
            content: None,
            source: None,
        }];

        let findings = run_rules(&file, &nodes);
        assert!(findings.iter().any(|f| f.rule_id == "prefer_slice"));
    }

    #[test]
    fn test_naming_non_snake_fn() {
        let nodes = vec![NodeInfo {
            id: "test-id".into(),
            kind: "fn".into(),
            name: Some("myFunc".into()),
            span_start: Some(1),
            content: None,
            source: None,
        }];

        let file = syn::parse_file("fn placeholder() {}").unwrap();
        let findings = run_rules(&file, &nodes);
        assert!(findings.iter().any(|f| f.rule_id == "non_snake_fn"));
    }

    #[test]
    fn test_missing_debug_derive() {
        let source = "struct Foo { x: i32 }";
        let file = syn::parse_file(source).unwrap();
        let nodes = vec![NodeInfo {
            id: "test-id".into(),
            kind: "struct".into(),
            name: Some("Foo".into()),
            span_start: Some(1),
            content: None,
            source: None,
        }];

        let findings = run_rules(&file, &nodes);
        assert!(findings.iter().any(|f| f.rule_id == "missing_derive_debug"));
    }

    #[test]
    fn test_has_debug_derive_no_finding() {
        let source = "#[derive(Debug)]\nstruct Foo { x: i32 }";
        let file = syn::parse_file(source).unwrap();
        let nodes = vec![NodeInfo {
            id: "test-id".into(),
            kind: "struct".into(),
            name: Some("Foo".into()),
            span_start: Some(2), // struct is on line 2
            content: None,
            source: None,
        }];

        let findings = run_rules(&file, &nodes);
        assert!(!findings.iter().any(|f| f.rule_id == "missing_derive_debug"));
    }
}
