use std::fmt;

use serde::{Deserialize, Serialize};

/// A typed pointer on the stack. Every stack item is a Ptr.
///
/// `kind` determines how the item is rendered and what methods dispatch on it.
/// `ref_id` holds the primary value (literal for scalars, UUID for references).
/// `meta` holds auxiliary data (e.g. list contents, workspace details).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ptr {
    pub kind: String,
    pub ref_id: String,
    #[serde(default)]
    pub meta: serde_json::Value,
    /// Stable database rowid (set when persisted, 0 for transient items).
    #[serde(default)]
    pub id: i64,
}

impl Ptr {
    pub fn int(n: i64) -> Self {
        Self {
            kind: "int".into(),
            ref_id: n.to_string(),
            meta: serde_json::Value::Null,
            id: 0,
        }
    }

    pub fn float(f: f64) -> Self {
        Self {
            kind: "float".into(),
            ref_id: f.to_string(),
            meta: serde_json::Value::Null,
            id: 0,
        }
    }

    pub fn text(s: &str) -> Self {
        Self {
            kind: "text".into(),
            ref_id: s.to_string(),
            meta: serde_json::Value::Null,
            id: 0,
        }
    }

    pub fn list(items: Vec<Ptr>) -> Self {
        Self {
            kind: "list".into(),
            ref_id: String::new(),
            meta: serde_json::to_value(items).unwrap_or_default(),
            id: 0,
        }
    }

    pub fn info(s: &str) -> Self {
        Self {
            kind: "text.info".into(),
            ref_id: s.to_string(),
            meta: serde_json::Value::Null,
            id: 0,
        }
    }

    pub fn warn(s: &str) -> Self {
        Self {
            kind: "text.warn".into(),
            ref_id: s.to_string(),
            meta: serde_json::Value::Null,
            id: 0,
        }
    }

    pub fn success(s: &str) -> Self {
        Self {
            kind: "text.success".into(),
            ref_id: s.to_string(),
            meta: serde_json::Value::Null,
            id: 0,
        }
    }

    pub fn muted(s: &str) -> Self {
        Self {
            kind: "text.muted".into(),
            ref_id: s.to_string(),
            meta: serde_json::Value::Null,
            id: 0,
        }
    }

    pub fn error(msg: &str) -> Self {
        Self {
            kind: "error".into(),
            ref_id: msg.to_string(),
            meta: serde_json::Value::Null,
            id: 0,
        }
    }

    pub fn help_list(items: Vec<serde_json::Value>) -> Self {
        Self {
            kind: "list.help".into(),
            ref_id: String::new(),
            meta: serde_json::json!({"items": items}),
            id: 0,
        }
    }

    pub fn library(name: &str) -> Self {
        Self {
            kind: "library".into(),
            ref_id: name.to_string(),
            meta: serde_json::Value::Null,
            id: 0,
        }
    }

    /// Try to extract an integer value from this Ptr.
    pub fn as_int(&self) -> Option<i64> {
        if self.kind == "int" {
            self.ref_id.parse().ok()
        } else {
            None
        }
    }

    /// Try to extract a float value from this Ptr.
    pub fn as_float(&self) -> Option<f64> {
        match self.kind.as_str() {
            "float" => self.ref_id.parse().ok(),
            "int" => self.ref_id.parse::<i64>().ok().map(|n| n as f64),
            _ => None,
        }
    }

    /// Check if this Ptr is numeric (int or float).
    pub fn is_numeric(&self) -> bool {
        matches!(self.kind.as_str(), "int" | "float")
    }
}

impl fmt::Display for Ptr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind.as_str() {
            "int" => write!(f, "{}", self.ref_id),
            "float" => {
                let s = &self.ref_id;
                if s.ends_with(".0") {
                    write!(f, "{}", &s[..s.len() - 2])
                } else {
                    write!(f, "{s}")
                }
            }
            "text" => {
                let s = &self.ref_id;
                if s.len() > 60 {
                    write!(f, "\"{}...\"", &s[..57])
                } else {
                    write!(f, "\"{}\"", s)
                }
            }
            "text.info" | "text.warn" | "text.success" | "text.muted" => {
                write!(f, "{}", self.ref_id)
            }
            "list" => {
                if let Ok(items) = serde_json::from_value::<Vec<Ptr>>(self.meta.clone()) {
                    let rendered: Vec<String> = items.iter().take(10).map(|p| p.to_string()).collect();
                    let suffix = if items.len() > 10 { "..." } else { "" };
                    write!(f, "[{}{}]", rendered.join(" "), suffix)
                } else {
                    write!(f, "[]")
                }
            }
            "workspace_list" => {
                if let Some(items) = self.meta.get("items").and_then(|v| v.as_array()) {
                    let lines: Vec<String> = items
                        .iter()
                        .enumerate()
                        .map(|(i, item)| {
                            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            let count = item.get("item_count").and_then(|v| v.as_i64()).unwrap_or(0);
                            let active = item.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false);
                            let marker = if active { " *" } else { "" };
                            format!("  {}. {} ({} items){}", i + 1, name, count, marker)
                        })
                        .collect();
                    write!(f, "workspaces:\n{}", lines.join("\n"))
                } else {
                    write!(f, "workspaces: (none)")
                }
            }
            "list.help" => {
                if let Some(items) = self.meta.get("items").and_then(|v| v.as_array()) {
                    let entries: Vec<(&str, &str)> = items.iter()
                        .filter_map(|item| {
                            let path = item.get("path")?.as_str()?;
                            let desc = item.get("desc")?.as_str()?;
                            Some((path, desc))
                        })
                        .collect();
                    if entries.is_empty() {
                        return write!(f, "commands: (none)");
                    }
                    let mut lines = vec!["commands:".to_string()];
                    // Track ancestor stack for nesting
                    let mut ancestors: Vec<&str> = Vec::new();
                    for (path, desc) in &entries {
                        // Pop ancestors that aren't a prefix of current path
                        while let Some(&top) = ancestors.last() {
                            if path.starts_with(top) && path.as_bytes().get(top.len()) == Some(&b'.') {
                                break;
                            }
                            ancestors.pop();
                        }
                        let depth = ancestors.len();
                        let indent = "  ".repeat(depth + 1);
                        let label = if depth > 0 {
                            // Show only the last segment prefixed with "."
                            let last_dot = path.rfind('.').unwrap();
                            format!(".{}", &path[last_dot + 1..])
                        } else {
                            path.to_string()
                        };
                        lines.push(format!("{}{} — {}", indent, label, desc));
                        ancestors.push(path);
                    }
                    write!(f, "{}", lines.join("\n"))
                } else {
                    write!(f, "commands: (none)")
                }
            }
            "session" => {
                let handle = self.meta.get("handle").and_then(|v| v.as_str()).unwrap_or("anonymous");
                let provider = self.meta.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
                write!(f, "session: {} ({})", handle, provider)
            }
            "auth_pending" => {
                let url = self.meta.get("url").and_then(|v| v.as_str()).unwrap_or("?");
                write!(f, "auth: redirecting to {}", url)
            }
            "error" => write!(f, "error: {}", self.ref_id),
            "library" => write!(f, "[{}]", self.ref_id),
            _ => write!(f, "{}:{}", self.kind, self.ref_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_display() {
        assert_eq!(Ptr::int(42).to_string(), "42");
    }

    #[test]
    fn float_display() {
        assert_eq!(Ptr::float(3.14).to_string(), "3.14");
    }

    #[test]
    fn float_integer_valued() {
        assert_eq!(Ptr::float(4.0).to_string(), "4");
    }

    #[test]
    fn text_display() {
        assert_eq!(Ptr::text("hello").to_string(), "\"hello\"");
    }

    #[test]
    fn error_display() {
        assert_eq!(Ptr::error("bad").to_string(), "error: bad");
    }

    #[test]
    fn list_display() {
        let list = Ptr::list(vec![Ptr::int(1), Ptr::int(2), Ptr::int(3)]);
        assert_eq!(list.to_string(), "[1 2 3]");
    }

    #[test]
    fn info_display() {
        assert_eq!(Ptr::info("authenticating").to_string(), "authenticating");
    }

    #[test]
    fn warn_display() {
        assert_eq!(Ptr::warn("session closed").to_string(), "session closed");
    }

    #[test]
    fn success_display() {
        assert_eq!(Ptr::success("done").to_string(), "done");
    }

    #[test]
    fn muted_display() {
        assert_eq!(Ptr::muted("hint").to_string(), "hint");
    }

    #[test]
    fn as_int() {
        assert_eq!(Ptr::int(42).as_int(), Some(42));
        assert_eq!(Ptr::text("x").as_int(), None);
    }

    #[test]
    fn as_float_promotion() {
        assert_eq!(Ptr::int(3).as_float(), Some(3.0));
        assert_eq!(Ptr::float(3.14).as_float(), Some(3.14));
    }
}
