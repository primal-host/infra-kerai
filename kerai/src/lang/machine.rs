use std::collections::HashMap;

use super::ptr::Ptr;
use super::token::{tokenize, TokenKind};

/// Handler function signature for stack machine commands.
pub type Handler = fn(machine: &mut Machine) -> Result<(), String>;

/// The stack machine: dispatches words against registered handlers and type methods.
/// Purely synchronous — async DB operations happen in the serve layer.
pub struct Machine {
    pub workspace_id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub stack: Vec<Ptr>,
    /// Global word handlers (e.g., "login", "workspace", "clear").
    handlers: HashMap<String, Handler>,
    /// Type-dispatched methods: (kind, word) → handler.
    /// For library dispatch: ("library:workspace", "list").
    type_methods: HashMap<(String, String), Handler>,
    /// One-liner help text. Keys: handler name or "library:X/method".
    help: HashMap<String, String>,
}

impl Machine {
    pub fn new(
        workspace_id: uuid::Uuid,
        user_id: uuid::Uuid,
        handlers: HashMap<String, Handler>,
        type_methods: HashMap<(String, String), Handler>,
        help: HashMap<String, String>,
    ) -> Self {
        Self {
            workspace_id,
            user_id,
            stack: Vec::new(),
            handlers,
            type_methods,
            help,
        }
    }

    /// Execute an input string through the stack machine.
    pub fn execute(&mut self, input: &str) -> Result<(), String> {
        let tokens = tokenize(input);
        let mut i = 0;

        while i < tokens.len() {
            let token = &tokens[i];
            i += 1;

            match token.kind {
                TokenKind::LBracket => {
                    // Collect list elements until matching RBracket
                    let mut depth = 1;
                    let mut list_tokens = Vec::new();
                    while i < tokens.len() && depth > 0 {
                        match tokens[i].kind {
                            TokenKind::LBracket => {
                                depth += 1;
                                list_tokens.push(tokens[i].clone());
                            }
                            TokenKind::RBracket => {
                                depth -= 1;
                                if depth > 0 {
                                    list_tokens.push(tokens[i].clone());
                                }
                            }
                            _ => list_tokens.push(tokens[i].clone()),
                        }
                        i += 1;
                    }
                    // Parse list elements as literals
                    let items: Vec<Ptr> = list_tokens
                        .iter()
                        .filter(|t| t.kind == TokenKind::Word)
                        .map(|t| parse_literal(&t.value, t.quoted))
                        .collect();
                    self.stack.push(Ptr::list(items));
                }
                TokenKind::RBracket | TokenKind::LParen | TokenKind::RParen => {
                    // Stray structural tokens — ignore
                }
                TokenKind::Word => {
                    let raw = &token.value;

                    // 1. Quoted strings are always text literals
                    if token.quoted {
                        self.stack.push(Ptr::text(raw));
                        continue;
                    }

                    // Detect trailing dot → help mode (e.g., "clear." or "admin user allow.")
                    // Skip for number-like bases so "42." still parses as float 42.0.
                    let (word, help_mode) = if raw.ends_with('.') && raw.len() > 1 {
                        let base = &raw[..raw.len() - 1];
                        if try_parse_number(base).is_some() {
                            (raw.as_str(), false)
                        } else {
                            (base, true)
                        }
                    } else {
                        (raw.as_str(), false)
                    };

                    // 2. Try parse as literal (int, float)
                    if let Some(ptr) = try_parse_number(word) {
                        self.stack.push(ptr);
                        continue;
                    }

                    // 3. help command — structured list, one-liner, or path lookup
                    if word == "help" {
                        if help_mode {
                            let msg = self.help.get("help")
                                .cloned()
                                .unwrap_or_else(|| "list all commands".into());
                            self.stack.push(Ptr::info(&msg));
                        } else {
                            self.push_help_list();
                        }
                        continue;
                    }
                    // help.X or X.help → look up one-liner
                    let help_path = word.strip_prefix("help.")
                        .or_else(|| word.strip_suffix(".help"));
                    if let Some(path) = help_path {
                        if !path.is_empty() {
                            let ptr = self.lookup_help_text(path);
                            self.stack.push(ptr);
                            continue;
                        }
                    }

                    // 4. Check global handlers
                    if let Some(handler) = self.handlers.get(word).copied() {
                        if help_mode {
                            let ptr = match self.help.get(word) {
                                Some(desc) => Ptr::info(desc),
                                None => Ptr::warn(&format!("{}: no help available", word)),
                            };
                            self.stack.push(ptr);
                            continue;
                        }
                        if let Err(e) = handler(self) {
                            self.stack.push(Ptr::error(&e));
                        }
                        continue;
                    }

                    // 5. Check dot-form: "a.b" → lookup as handler
                    if word.contains('.') {
                        if let Some(handler) = self.handlers.get(word).copied() {
                            if help_mode {
                                let ptr = match self.help.get(word) {
                                    Some(desc) => Ptr::info(desc),
                                    None => Ptr::warn(&format!("{}: no help available", word)),
                                };
                                self.stack.push(ptr);
                                continue;
                            }
                            if let Err(e) = handler(self) {
                                self.stack.push(Ptr::error(&e));
                            }
                            continue;
                        }
                    }

                    // 6. If stack top is a library, dispatch as library method
                    if let Some(top) = self.stack.last() {
                        if top.kind == "library" {
                            let lib_ref = top.ref_id.clone();
                            let lib_key = format!("library:{}", lib_ref);

                            // "man" — list all methods for this library
                            if word == "man" {
                                self.stack.pop();
                                self.push_library_man(&lib_ref, &lib_key);
                                continue;
                            }

                            let method_key = (lib_key.clone(), word.to_string());
                            if let Some(handler) = self.type_methods.get(&method_key).copied() {
                                // Pop the library marker before dispatching
                                self.stack.pop();
                                if help_mode {
                                    let help_key = format!("{}/{}", lib_key, word);
                                    let ptr = match self.help.get(&help_key) {
                                        Some(desc) => Ptr::info(desc),
                                        None => Ptr::warn(&format!("{}.{}: no help available", lib_ref, word)),
                                    };
                                    self.stack.push(ptr);
                                    continue;
                                }
                                if let Err(e) = handler(self) {
                                    self.stack.push(Ptr::error(&e));
                                }
                                continue;
                            }
                        }
                    }

                    // 7. Check type methods on stack top
                    if let Some(top) = self.stack.last() {
                        let type_key = (top.kind.clone(), word.to_string());
                        if let Some(handler) = self.type_methods.get(&type_key).copied() {
                            if let Err(e) = handler(self) {
                                self.stack.push(Ptr::error(&e));
                            }
                            continue;
                        }
                    }

                    // 8. Unknown word — push as error
                    self.stack.push(Ptr::error(&format!("unknown word: {word}")));
                }
            }
        }

        Ok(())
    }

    /// Push a Ptr onto the stack.
    pub fn push(&mut self, ptr: Ptr) {
        self.stack.push(ptr);
    }

    /// Pop the top Ptr from the stack.
    pub fn pop(&mut self) -> Option<Ptr> {
        self.stack.pop()
    }

    /// Peek at the top of the stack.
    pub fn peek(&self) -> Option<&Ptr> {
        self.stack.last()
    }

    /// Stack depth.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Push a manual page for a library: lists all its methods with help text.
    fn push_library_man(&mut self, lib_ref: &str, lib_key: &str) {
        let prefix = format!("{}/", lib_key);
        let mut methods: Vec<(&str, &str)> = self.help.iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(k, v)| (&k[prefix.len()..], v.as_str()))
            .collect();
        methods.sort_by_key(|(name, _)| *name);

        let mut lines = vec![format!("{}:", lib_ref)];
        if methods.is_empty() {
            lines.push("  (no documented methods)".to_string());
        } else {
            let max_len = methods.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
            for (method, desc) in &methods {
                lines.push(format!("  .{:<width$} — {}", method, desc, width = max_len));
            }
        }
        self.stack.push(Ptr::text(&lines.join("\n")));
    }

    /// Look up help text for a dot-path like "admin.user.allow".
    /// Tries direct handler key first, then library key format.
    /// Returns info Ptr on match, warn Ptr on miss.
    fn lookup_help_text(&self, path: &str) -> Ptr {
        // Direct match (global handlers: "dup", "clear", "admin", etc.)
        if let Some(desc) = self.help.get(path) {
            return Ptr::info(desc);
        }
        // Library format: "admin.user.allow" → "library:admin.user/allow"
        if let Some(dot_pos) = path.rfind('.') {
            let lib_part = &path[..dot_pos];
            let method = &path[dot_pos + 1..];
            let key = format!("library:{}/{}", lib_part, method);
            if let Some(desc) = self.help.get(&key) {
                return Ptr::info(desc);
            }
        }
        Ptr::warn(&format!("{}: no help available", path))
    }

    /// Push a structured list of all registered commands as a `list.help` Ptr.
    fn push_help_list(&mut self) {
        let mut items: Vec<serde_json::Value> = self.help.iter()
            .map(|(key, desc)| {
                // Convert internal key format to dot-path:
                //   "library:admin.user/allow" → "admin.user.allow"
                //   "library:admin/oauth"      → "admin.oauth"
                //   "dup"                       → "dup"
                let path = if let Some(rest) = key.strip_prefix("library:") {
                    rest.replace('/', ".")
                } else {
                    key.clone()
                };
                serde_json::json!({"path": path, "desc": desc})
            })
            .collect();
        items.sort_by(|a, b| {
            let pa = a["path"].as_str().unwrap_or("");
            let pb = b["path"].as_str().unwrap_or("");
            pa.cmp(pb)
        });
        self.stack.push(Ptr::help_list(items));
    }
}

/// Parse a token value as a literal Ptr (int or float).
fn try_parse_number(s: &str) -> Option<Ptr> {
    // Hex literal
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        if let Ok(n) = i64::from_str_radix(hex, 16) {
            return Some(Ptr::int(n));
        }
    }
    if let Ok(n) = s.parse::<i64>() {
        return Some(Ptr::int(n));
    }
    if let Ok(f) = s.parse::<f64>() {
        return Some(Ptr::float(f));
    }
    None
}

/// Parse a literal value — if it's a number, make it numeric; otherwise text.
fn parse_literal(s: &str, quoted: bool) -> Ptr {
    if quoted {
        return Ptr::text(s);
    }
    try_parse_number(s).unwrap_or_else(|| Ptr::text(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::handlers;

    fn test_machine() -> Machine {
        let (handlers, type_methods, help) = handlers::register_all();
        Machine::new(uuid::Uuid::nil(), uuid::Uuid::nil(), handlers, type_methods, help)
    }

    #[test]
    fn push_integers() {
        let mut m = test_machine();
        m.execute("42 7").unwrap();
        assert_eq!(m.stack.len(), 2);
        assert_eq!(m.stack[0], Ptr::int(42));
        assert_eq!(m.stack[1], Ptr::int(7));
    }

    #[test]
    fn push_float() {
        let mut m = test_machine();
        m.execute("3.14").unwrap();
        assert_eq!(m.stack[0].kind, "float");
    }

    #[test]
    fn push_hex() {
        let mut m = test_machine();
        m.execute("0xFF").unwrap();
        assert_eq!(m.stack[0], Ptr::int(255));
    }

    #[test]
    fn push_quoted_string() {
        let mut m = test_machine();
        m.execute("\"hello world\"").unwrap();
        assert_eq!(m.stack[0], Ptr::text("hello world"));
    }

    #[test]
    fn push_list() {
        let mut m = test_machine();
        m.execute("[1 2 3]").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "list");
    }

    #[test]
    fn arithmetic_add() {
        let mut m = test_machine();
        m.execute("3 4 +").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0], Ptr::int(7));
    }

    #[test]
    fn arithmetic_mixed() {
        let mut m = test_machine();
        m.execute("3 4.0 +").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "float");
        assert_eq!(m.stack[0].as_float(), Some(7.0));
    }

    #[test]
    fn dup_top() {
        let mut m = test_machine();
        m.execute("42 dup").unwrap();
        assert_eq!(m.stack.len(), 2);
        assert_eq!(m.stack[0], Ptr::int(42));
        assert_eq!(m.stack[1], Ptr::int(42));
    }

    #[test]
    fn drop_top() {
        let mut m = test_machine();
        m.execute("1 2 3 drop").unwrap();
        assert_eq!(m.stack.len(), 2);
    }

    #[test]
    fn swap_top_two() {
        let mut m = test_machine();
        m.execute("1 2 swap").unwrap();
        assert_eq!(m.stack[0], Ptr::int(2));
        assert_eq!(m.stack[1], Ptr::int(1));
    }

    #[test]
    fn clear_stack() {
        let mut m = test_machine();
        m.execute("1 2 3 clear").unwrap();
        assert!(m.stack.is_empty());
    }

    #[test]
    fn unknown_word_error() {
        let mut m = test_machine();
        m.execute("frobnicate").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "error");
    }

    #[test]
    fn library_dispatch() {
        let mut m = test_machine();
        m.execute("workspace").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "library");
        assert_eq!(m.stack[0].ref_id, "workspace");
    }

    #[test]
    fn division_by_zero() {
        let mut m = test_machine();
        m.execute("1 0 /").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "error");
    }

    #[test]
    fn workspace_list_dispatch() {
        let mut m = test_machine();
        m.execute("workspace list").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "workspace_list_request");
    }

    #[test]
    fn login_bsky_dispatch() {
        let mut m = test_machine();
        m.execute("login bsky").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "auth_pending_request");
    }

    #[test]
    fn chained_arithmetic() {
        let mut m = test_machine();
        // RPN: 2 3 + 4 * = (2+3)*4 = 20
        m.execute("2 3 + 4 *").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0], Ptr::int(20));
    }

    #[test]
    fn help_global_handler() {
        let mut m = test_machine();
        m.execute("clear.").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "clear the stack");
    }

    #[test]
    fn help_library_pusher() {
        let mut m = test_machine();
        m.execute("admin.").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "administration commands");
    }

    #[test]
    fn help_library_method() {
        let mut m = test_machine();
        m.execute("admin user allow.").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "allowlist a bsky handle for login");
    }

    #[test]
    fn help_does_not_execute() {
        // "allow." should show help, not try to pop a handle from the stack
        let mut m = test_machine();
        m.execute("admin user allow.").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info"); // help text, not error
    }

    #[test]
    fn help_preserves_float() {
        // "42." should parse as float 42.0, not trigger help mode
        let mut m = test_machine();
        m.execute("42.").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "float");
    }

    #[test]
    fn help_pushes_list_help() {
        let mut m = test_machine();
        m.execute("help").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "list.help");
        let items = m.stack[0].meta.get("items").unwrap().as_array().unwrap();
        // Should contain all registered commands
        assert!(items.len() > 10);
        // Items should be sorted by path
        let paths: Vec<&str> = items.iter().map(|i| i["path"].as_str().unwrap()).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted);
        // Check a few known entries
        assert!(items.iter().any(|i| i["path"] == "clear" && i["desc"] == "clear the stack"));
        assert!(items.iter().any(|i| i["path"] == "admin.user.allow"));
        assert!(items.iter().any(|i| i["path"] == "help" && i["desc"] == "list all commands"));
    }

    #[test]
    fn help_dot_shows_help_text() {
        let mut m = test_machine();
        m.execute("help.").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "list all commands");
    }

    #[test]
    fn help_dot_path_global() {
        let mut m = test_machine();
        m.execute("help.clear").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "clear the stack");
    }

    #[test]
    fn help_dot_path_library() {
        let mut m = test_machine();
        m.execute("help.admin").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "administration commands");
    }

    #[test]
    fn help_dot_path_nested_method() {
        let mut m = test_machine();
        m.execute("help.admin.user.allow").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "allowlist a bsky handle for login");
    }

    #[test]
    fn help_dot_path_deep_nested() {
        let mut m = test_machine();
        m.execute("help.admin.oauth.setup.bsky").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "generate ES256 keypair for Bluesky OAuth");
    }

    #[test]
    fn help_dot_path_unknown() {
        let mut m = test_machine();
        m.execute("help.nonexistent").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.warn");
        assert_eq!(m.stack[0].ref_id, "nonexistent: no help available");
    }

    #[test]
    fn suffix_help_global() {
        let mut m = test_machine();
        m.execute("clear.help").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "clear the stack");
    }

    #[test]
    fn suffix_help_library() {
        let mut m = test_machine();
        m.execute("admin.help").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "administration commands");
    }

    #[test]
    fn suffix_help_nested_method() {
        let mut m = test_machine();
        m.execute("admin.user.allow.help").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
        assert_eq!(m.stack[0].ref_id, "allowlist a bsky handle for login");
    }

    #[test]
    fn man_library() {
        let mut m = test_machine();
        m.execute("admin man").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text");
        assert!(m.stack[0].ref_id.contains("admin:"));
        assert!(m.stack[0].ref_id.contains(".oauth"));
        assert!(m.stack[0].ref_id.contains(".user"));
    }

    #[test]
    fn man_nested_library() {
        let mut m = test_machine();
        m.execute("admin user man").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text");
        assert!(m.stack[0].ref_id.contains("admin.user:"));
        assert!(m.stack[0].ref_id.contains(".allow"));
    }
}
