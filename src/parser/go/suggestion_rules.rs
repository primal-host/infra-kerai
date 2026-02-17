/// Go-specific suggestion rules — detect code patterns that merit advisory comments.
///
/// Reuses the Finding struct pattern from the Rust suggestion system.

/// A suggestion finding from a Go rule.
#[derive(Debug, Clone)]
pub struct GoFinding {
    pub rule_id: &'static str,
    pub message: String,
    pub severity: &'static str,
    pub category: &'static str,
    pub line: i32,
    pub target_node_id: String,
}

/// Go node information for rule analysis.
#[derive(Debug, Clone)]
pub struct GoNodeInfo {
    pub id: String,
    pub kind: String,
    pub name: Option<String>,
    pub span_start: Option<i32>,
    pub exported: bool,
    pub has_doc: bool,
    pub returns: Option<String>,
}

/// Run all Go suggestion rules.
pub fn run_go_rules(nodes: &[GoNodeInfo], package_name: Option<&str>) -> Vec<GoFinding> {
    let mut findings = Vec::new();

    check_exported_no_doc(nodes, &mut findings);
    check_error_not_last(nodes, &mut findings);
    if let Some(pkg) = package_name {
        check_stutter(nodes, pkg, &mut findings);
    }

    findings
}

/// Exported symbol has no doc comment above it.
fn check_exported_no_doc(nodes: &[GoNodeInfo], findings: &mut Vec<GoFinding>) {
    for node in nodes {
        if !node.exported {
            continue;
        }

        let documentable = matches!(
            node.kind.as_str(),
            "go_func" | "go_method" | "go_type_spec" | "go_var_spec" | "go_const_spec"
        );
        if !documentable {
            continue;
        }

        if !node.has_doc {
            let name = node.name.as_deref().unwrap_or("?");
            let line = node.span_start.unwrap_or(0);
            findings.push(GoFinding {
                rule_id: "go_exported_no_doc",
                message: format!("exported symbol `{}` has no doc comment", name),
                severity: "info",
                category: "naming",
                line,
                target_node_id: node.id.clone(),
            });
        }
    }
}

/// Function returns error but not as last value.
fn check_error_not_last(nodes: &[GoNodeInfo], findings: &mut Vec<GoFinding>) {
    for node in nodes {
        let returns = match &node.returns {
            Some(r) => r,
            None => continue,
        };

        if !matches!(node.kind.as_str(), "go_func" | "go_method") {
            continue;
        }

        // Parse return types: strip parens, split by comma
        let trimmed = returns.trim().trim_start_matches('(').trim_end_matches(')');
        let parts: Vec<&str> = trimmed.split(',').map(|s| s.trim()).collect();

        if parts.len() < 2 {
            continue;
        }

        // Check if any non-last return value is "error"
        let has_non_last_error = parts[..parts.len() - 1]
            .iter()
            .any(|p| *p == "error" || p.ends_with(" error"));

        if has_non_last_error {
            let name = node.name.as_deref().unwrap_or("?");
            let line = node.span_start.unwrap_or(0);
            findings.push(GoFinding {
                rule_id: "go_error_not_last",
                message: format!(
                    "function `{}` returns error in non-last position",
                    name
                ),
                severity: "warning",
                category: "idiom",
                line,
                target_node_id: node.id.clone(),
            });
        }
    }
}

/// Type name stutters with package name (e.g., http.HTTPClient → http.Client).
fn check_stutter(nodes: &[GoNodeInfo], package_name: &str, findings: &mut Vec<GoFinding>) {
    let pkg_lower = package_name.to_lowercase();

    for node in nodes {
        if node.kind != "go_type_spec" || !node.exported {
            continue;
        }

        let name = match &node.name {
            Some(n) => n,
            None => continue,
        };

        let name_lower = name.to_lowercase();

        // Check if type name starts with package name (case-insensitive)
        if name_lower.starts_with(&pkg_lower) && name_lower.len() > pkg_lower.len() {
            let suffix = &name[pkg_lower.len()..];
            // Only flag if what remains starts with uppercase (genuine stutter)
            if suffix.starts_with(|c: char| c.is_uppercase()) {
                let line = node.span_start.unwrap_or(0);
                findings.push(GoFinding {
                    rule_id: "go_stutter",
                    message: format!(
                        "type `{}.{}` stutters; consider `{}.{}`",
                        package_name, name, package_name, suffix
                    ),
                    severity: "info",
                    category: "naming",
                    line,
                    target_node_id: node.id.clone(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(kind: &str, name: &str, exported: bool, has_doc: bool) -> GoNodeInfo {
        GoNodeInfo {
            id: format!("test-{}", name),
            kind: kind.to_string(),
            name: Some(name.to_string()),
            span_start: Some(1),
            exported,
            has_doc,
            returns: None,
        }
    }

    #[test]
    fn test_exported_no_doc() {
        let nodes = vec![
            make_node("go_func", "Hello", true, false),
            make_node("go_func", "hello", false, false),
        ];
        let findings = run_go_rules(&nodes, Some("main"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "go_exported_no_doc");
    }

    #[test]
    fn test_exported_with_doc_no_finding() {
        let nodes = vec![make_node("go_func", "Hello", true, true)];
        let findings = run_go_rules(&nodes, Some("main"));
        assert!(
            !findings.iter().any(|f| f.rule_id == "go_exported_no_doc"),
            "documented export should not trigger"
        );
    }

    #[test]
    fn test_error_not_last() {
        let mut node = make_node("go_func", "BadFunc", true, true);
        node.returns = Some("(error, string)".to_string());
        let findings = run_go_rules(&[node], Some("main"));
        assert!(findings.iter().any(|f| f.rule_id == "go_error_not_last"));
    }

    #[test]
    fn test_error_last_no_finding() {
        let mut node = make_node("go_func", "GoodFunc", true, true);
        node.returns = Some("(string, error)".to_string());
        let findings = run_go_rules(&[node], Some("main"));
        assert!(!findings.iter().any(|f| f.rule_id == "go_error_not_last"));
    }

    #[test]
    fn test_stutter() {
        let nodes = vec![make_node("go_type_spec", "HttpClient", true, true)];
        let findings = run_go_rules(&nodes, Some("http"));
        assert!(findings.iter().any(|f| f.rule_id == "go_stutter"));
    }

    #[test]
    fn test_no_stutter() {
        let nodes = vec![make_node("go_type_spec", "Client", true, true)];
        let findings = run_go_rules(&nodes, Some("http"));
        assert!(!findings.iter().any(|f| f.rule_id == "go_stutter"));
    }
}
