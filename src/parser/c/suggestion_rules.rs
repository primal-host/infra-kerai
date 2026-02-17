/// C-specific suggestion rules — detect code patterns that merit advisory comments.
use super::kinds;

/// A suggestion finding from a C rule.
#[derive(Debug, Clone)]
pub struct CFinding {
    pub rule_id: &'static str,
    pub message: String,
    pub severity: &'static str,
    pub category: &'static str,
    pub line: i32,
    pub target_node_id: String,
}

/// C node information for rule analysis.
#[derive(Debug, Clone)]
pub struct CNodeInfo {
    pub id: String,
    pub kind: String,
    pub name: Option<String>,
    pub span_start: Option<i32>,
    pub span_end: Option<i32>,
    pub is_static: bool,
    #[allow(dead_code)]
    pub has_doc: bool,
    pub params: Option<String>,
    pub is_header: bool,
}

/// Run all C suggestion rules.
pub fn run_c_rules(nodes: &[CNodeInfo]) -> Vec<CFinding> {
    let mut findings = Vec::new();

    check_void_param(nodes, &mut findings);
    check_global_no_static(nodes, &mut findings);
    check_magic_number(nodes, &mut findings);
    check_long_function(nodes, &mut findings);
    check_missing_include_guard(nodes, &mut findings);

    findings
}

/// `func()` should be `func(void)` for explicit zero-arg declaration in C.
fn check_void_param(nodes: &[CNodeInfo], findings: &mut Vec<CFinding>) {
    for node in nodes {
        if node.kind != kinds::C_FUNCTION {
            continue;
        }

        if let Some(ref params) = node.params {
            let trimmed = params.trim();
            if trimmed == "()" {
                let name = node.name.as_deref().unwrap_or("?");
                let line = node.span_start.unwrap_or(0);
                findings.push(CFinding {
                    rule_id: "c_no_void_param",
                    message: format!(
                        "function `{}` uses `()` — prefer `(void)` for explicit zero-arg declaration",
                        name
                    ),
                    severity: "info",
                    category: "idiom",
                    line,
                    target_node_id: node.id.clone(),
                });
            }
        }
    }
}

/// Non-static file-scope variable — consider `static` for internal linkage.
fn check_global_no_static(nodes: &[CNodeInfo], findings: &mut Vec<CFinding>) {
    for node in nodes {
        if node.kind != kinds::C_DECLARATION {
            continue;
        }

        if node.is_static {
            continue;
        }

        let name = match &node.name {
            Some(n) => n,
            None => continue,
        };

        let line = node.span_start.unwrap_or(0);
        findings.push(CFinding {
            rule_id: "c_global_no_static",
            message: format!(
                "file-scope variable `{}` is not `static` — consider internal linkage",
                name
            ),
            severity: "info",
            category: "visibility",
            line,
            target_node_id: node.id.clone(),
        });
    }
}

/// Numeric literal in expression (not in #define or enum) — consider named constant.
fn check_magic_number(nodes: &[CNodeInfo], findings: &mut Vec<CFinding>) {
    // This rule operates on all nodes, looking for number literals
    // that are children of expressions (not defines or enumerators).
    // Since we only have top-level node info, we skip this for now
    // and flag it as a placeholder for deeper analysis.
    let _ = (nodes, findings);
}

/// Function body exceeds 50 lines.
fn check_long_function(nodes: &[CNodeInfo], findings: &mut Vec<CFinding>) {
    for node in nodes {
        if node.kind != kinds::C_FUNCTION {
            continue;
        }

        let start = node.span_start.unwrap_or(0);
        let end = node.span_end.unwrap_or(0);
        let length = end - start;

        if length > 50 {
            let name = node.name.as_deref().unwrap_or("?");
            findings.push(CFinding {
                rule_id: "c_long_function",
                message: format!(
                    "function `{}` is {} lines long — consider splitting",
                    name, length
                ),
                severity: "warning",
                category: "complexity",
                line: start,
                target_node_id: node.id.clone(),
            });
        }
    }
}

/// Header file without `#ifndef`/`#define` guard pattern.
fn check_missing_include_guard(nodes: &[CNodeInfo], findings: &mut Vec<CFinding>) {
    // Check if this is a header file
    let is_header = nodes.iter().any(|n| n.is_header);
    if !is_header {
        return;
    }

    // Look for an ifdef/if_directive as the first non-include, non-comment node
    let has_guard = nodes
        .iter()
        .any(|n| n.kind == kinds::C_IFDEF || n.kind == kinds::C_IF_DIRECTIVE);

    if !has_guard {
        findings.push(CFinding {
            rule_id: "c_missing_include_guard",
            message: "header file missing `#ifndef`/`#define` include guard".to_string(),
            severity: "info",
            category: "idiom",
            line: 1,
            target_node_id: nodes
                .first()
                .map(|n| n.id.clone())
                .unwrap_or_default(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(kind: &str, name: &str) -> CNodeInfo {
        CNodeInfo {
            id: format!("test-{}", name),
            kind: kind.to_string(),
            name: Some(name.to_string()),
            span_start: Some(1),
            span_end: Some(10),
            is_static: false,
            has_doc: false,
            params: None,
            is_header: false,
        }
    }

    #[test]
    fn test_void_param() {
        let mut node = make_node(kinds::C_FUNCTION, "foo");
        node.params = Some("()".to_string());
        let findings = run_c_rules(&[node]);
        assert!(findings.iter().any(|f| f.rule_id == "c_no_void_param"));
    }

    #[test]
    fn test_void_param_explicit() {
        let mut node = make_node(kinds::C_FUNCTION, "foo");
        node.params = Some("(void)".to_string());
        let findings = run_c_rules(&[node]);
        assert!(!findings.iter().any(|f| f.rule_id == "c_no_void_param"));
    }

    #[test]
    fn test_global_no_static() {
        let node = make_node(kinds::C_DECLARATION, "counter");
        let findings = run_c_rules(&[node]);
        assert!(findings.iter().any(|f| f.rule_id == "c_global_no_static"));
    }

    #[test]
    fn test_global_static_no_finding() {
        let mut node = make_node(kinds::C_DECLARATION, "counter");
        node.is_static = true;
        let findings = run_c_rules(&[node]);
        assert!(!findings.iter().any(|f| f.rule_id == "c_global_no_static"));
    }

    #[test]
    fn test_long_function() {
        let mut node = make_node(kinds::C_FUNCTION, "big_func");
        node.span_start = Some(1);
        node.span_end = Some(60);
        let findings = run_c_rules(&[node]);
        assert!(findings.iter().any(|f| f.rule_id == "c_long_function"));
    }

    #[test]
    fn test_short_function_no_finding() {
        let mut node = make_node(kinds::C_FUNCTION, "small_func");
        node.span_start = Some(1);
        node.span_end = Some(20);
        let findings = run_c_rules(&[node]);
        assert!(!findings.iter().any(|f| f.rule_id == "c_long_function"));
    }

    #[test]
    fn test_missing_include_guard() {
        let mut node = make_node(kinds::C_INCLUDE, "stdio.h");
        node.is_header = true;
        let findings = run_c_rules(&[node]);
        assert!(findings.iter().any(|f| f.rule_id == "c_missing_include_guard"));
    }
}
