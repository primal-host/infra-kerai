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

                    // 3b. Stack manipulation: drop, fold, view with targeting
                    let stack_cmd = if word == "drop" || word.starts_with("drop.") {
                        Some(("drop", word.strip_prefix("drop.").unwrap_or("")))
                    } else if word == "fold" || word.starts_with("fold.") {
                        Some(("fold", word.strip_prefix("fold.").unwrap_or("")))
                    } else if word == "view" || word.starts_with("view.") {
                        Some(("view", word.strip_prefix("view.").unwrap_or("")))
                    } else {
                        None
                    };
                    if let Some((cmd, arg)) = stack_cmd {
                        // cmd.X. (help_mode on a targeted form) → show help
                        if !arg.is_empty() && help_mode {
                            let ptr = match self.help.get(cmd) {
                                Some(desc) => Ptr::info(desc),
                                None => Ptr::warn(&format!("{}: no help available", cmd)),
                            };
                            self.stack.push(ptr);
                            continue;
                        }
                        // Bare cmd → target top; cmd. (help_mode) → target all
                        let effective = if arg.is_empty() && !help_mode {
                            "0"
                        } else if arg.is_empty() {
                            ""
                        } else {
                            arg
                        };
                        match self.resolve_stack_targets(effective) {
                            Ok(targets) => match cmd {
                                "drop" => self.apply_drop(targets),
                                "fold" => self.apply_fold(&targets),
                                "view" => self.apply_view(&targets),
                                _ => unreachable!(),
                            },
                            Err(e) => {
                                self.stack.push(Ptr::error(&format!("{}: {}", cmd, e)));
                            }
                        }
                        continue;
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

    /// Resolve a targeting argument into Vec indices.
    ///
    /// Argument forms:
    ///   ""     → all items
    ///   "0"    → top item
    ///   "-N"   → position N from top (-1 = second)
    ///   "A-B"  → range of positions A through B from top (inclusive)
    ///   "N"    → item with rowid N (positive, non-zero)
    fn resolve_stack_targets(&self, arg: &str) -> Result<Vec<usize>, String> {
        if arg.is_empty() {
            return Ok((0..self.stack.len()).collect());
        }
        if let Some(neg) = arg.strip_prefix('-') {
            let n: usize = neg.parse().map_err(|_| format!("invalid index: -{}", neg))?;
            if n >= self.stack.len() {
                return Err(format!("position -{} out of range (depth {})", n, self.stack.len()));
            }
            return Ok(vec![self.stack.len() - 1 - n]);
        }
        if let Some(dash) = arg.find('-') {
            let start: usize = arg[..dash].parse()
                .map_err(|_| format!("invalid range: {}", arg))?;
            let end: usize = arg[dash + 1..].parse()
                .map_err(|_| format!("invalid range: {}", arg))?;
            if start > end {
                return Err("range start must be <= end".into());
            }
            if end >= self.stack.len() {
                return Err(format!("position {} out of range (depth {})", end, self.stack.len()));
            }
            let first_idx = self.stack.len() - 1 - end;
            let count = end - start + 1;
            return Ok((first_idx..first_idx + count).collect());
        }
        let n: i64 = arg.parse().map_err(|_| format!("invalid argument: {}", arg))?;
        if n == 0 {
            if self.stack.is_empty() {
                return Err("stack empty".into());
            }
            return Ok(vec![self.stack.len() - 1]);
        }
        if n > 0 {
            let pos = self.stack.iter().position(|p| p.id == n)
                .ok_or_else(|| format!("rowid {} not found", n))?;
            return Ok(vec![pos]);
        }
        Err(format!("invalid argument: {}", arg))
    }

    /// Remove items at the given Vec indices.
    fn apply_drop(&mut self, mut targets: Vec<usize>) {
        targets.sort_unstable();
        targets.dedup();
        for idx in targets.into_iter().rev() {
            self.stack.remove(idx);
        }
    }

    /// Set folded=true on items at the given Vec indices.
    /// Skips items whose meta is an array (e.g. list kind) to avoid data loss.
    fn apply_fold(&mut self, targets: &[usize]) {
        for &idx in targets {
            if let Some(item) = self.stack.get_mut(idx) {
                if item.meta.is_array() {
                    continue; // list items already single-line, skip
                }
                if let Some(obj) = item.meta.as_object_mut() {
                    obj.insert("folded".into(), serde_json::Value::Bool(true));
                    obj.remove("view");
                } else {
                    item.meta = serde_json::json!({"folded": true});
                }
            }
        }
    }

    /// Set view=true (unfold) on items at the given Vec indices.
    fn apply_view(&mut self, targets: &[usize]) {
        for &idx in targets {
            if let Some(item) = self.stack.get_mut(idx) {
                if item.meta.is_array() {
                    continue;
                }
                if let Some(obj) = item.meta.as_object_mut() {
                    obj.insert("view".into(), serde_json::Value::Bool(true));
                    obj.remove("folded");
                } else {
                    item.meta = serde_json::json!({"view": true});
                }
            }
        }
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
    fn drop_dot_zero() {
        let mut m = test_machine();
        m.execute("1 2 3 drop.0").unwrap();
        assert_eq!(m.stack.len(), 2);
        assert_eq!(m.stack[1], Ptr::int(2));
    }

    #[test]
    fn drop_negative_index() {
        let mut m = test_machine();
        m.execute("10 20 30 drop.-1").unwrap();
        assert_eq!(m.stack.len(), 2);
        // Removed second from top (20), leaving [10, 30]
        assert_eq!(m.stack[0], Ptr::int(10));
        assert_eq!(m.stack[1], Ptr::int(30));
    }

    #[test]
    fn drop_negative_deep() {
        let mut m = test_machine();
        m.execute("10 20 30 40 drop.-3").unwrap();
        assert_eq!(m.stack.len(), 3);
        // Removed 4th from top (10), leaving [20, 30, 40]
        assert_eq!(m.stack[0], Ptr::int(20));
        assert_eq!(m.stack[1], Ptr::int(30));
        assert_eq!(m.stack[2], Ptr::int(40));
    }

    #[test]
    fn drop_negative_out_of_range() {
        let mut m = test_machine();
        m.execute("10 20 drop.-5").unwrap();
        assert_eq!(m.stack.len(), 3); // 10, 20, + error
        assert_eq!(m.stack[2].kind, "error");
    }

    #[test]
    fn drop_range() {
        let mut m = test_machine();
        m.execute("10 20 30 40 50 drop.0-1").unwrap();
        assert_eq!(m.stack.len(), 3);
        // Removed top two (50, 40), leaving [10, 20, 30]
        assert_eq!(m.stack[0], Ptr::int(10));
        assert_eq!(m.stack[1], Ptr::int(20));
        assert_eq!(m.stack[2], Ptr::int(30));
    }

    #[test]
    fn drop_range_middle() {
        let mut m = test_machine();
        m.execute("10 20 30 40 50 drop.2-3").unwrap();
        assert_eq!(m.stack.len(), 3);
        // Removed positions 2,3 from top (30, 20), leaving [10, 40, 50]
        assert_eq!(m.stack[0], Ptr::int(10));
        assert_eq!(m.stack[1], Ptr::int(40));
        assert_eq!(m.stack[2], Ptr::int(50));
    }

    #[test]
    fn drop_range_all() {
        let mut m = test_machine();
        m.execute("10 20 30 drop.0-2").unwrap();
        assert!(m.stack.is_empty());
    }

    #[test]
    fn drop_range_out_of_range() {
        let mut m = test_machine();
        m.execute("10 20 drop.0-5").unwrap();
        assert_eq!(m.stack.len(), 3); // 10, 20, + error
        assert_eq!(m.stack[2].kind, "error");
    }

    #[test]
    fn drop_by_rowid() {
        let mut m = test_machine();
        // Simulate persisted items with rowids
        m.stack.push(Ptr { id: 100, ..Ptr::int(10) });
        m.stack.push(Ptr { id: 200, ..Ptr::int(20) });
        m.stack.push(Ptr { id: 300, ..Ptr::int(30) });
        m.execute("drop.200").unwrap();
        assert_eq!(m.stack.len(), 2);
        assert_eq!(m.stack[0].id, 100);
        assert_eq!(m.stack[1].id, 300);
    }

    #[test]
    fn drop_rowid_not_found() {
        let mut m = test_machine();
        m.execute("10 20 drop.99999").unwrap();
        assert_eq!(m.stack.len(), 3); // 10, 20, + error
        assert_eq!(m.stack[2].kind, "error");
    }

    #[test]
    fn drop_help_mode() {
        let mut m = test_machine();
        m.execute("drop.0.").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert_eq!(m.stack[0].kind, "text.info");
    }

    #[test]
    fn drop_dot_wipes_stack() {
        let mut m = test_machine();
        m.execute("1 2 3 drop.").unwrap();
        assert!(m.stack.is_empty());
    }

    #[test]
    fn fold_top() {
        let mut m = test_machine();
        m.execute("help fold").unwrap();
        assert_eq!(m.stack.len(), 1);
        assert!(m.stack[0].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
        // Folded list.help should display as one-line summary
        let display = m.stack[0].to_string();
        assert!(display.starts_with("[commands:"));
    }

    #[test]
    fn fold_dot_folds_all() {
        let mut m = test_machine();
        m.execute("help").unwrap();
        m.execute("42").unwrap();
        m.execute("fold.").unwrap();
        // help (list.help) should be folded
        assert!(m.stack[0].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
        // int has null meta → gets {"folded": true}
        assert!(m.stack[1].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
    }

    #[test]
    fn fold_by_position() {
        let mut m = test_machine();
        m.execute("10 20 30 fold.-2").unwrap();
        // Only the bottom item (10) should be folded
        assert!(m.stack[0].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
        assert!(!m.stack[1].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
        assert!(!m.stack[2].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
    }

    #[test]
    fn view_unfolds() {
        let mut m = test_machine();
        m.execute("help fold view").unwrap();
        assert_eq!(m.stack.len(), 1);
        // Should be unfolded (view=true, no folded)
        assert!(m.stack[0].meta.get("view").and_then(|v| v.as_bool()).unwrap_or(false));
        assert!(!m.stack[0].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
    }

    #[test]
    fn view_dot_unfolds_all() {
        let mut m = test_machine();
        m.execute("help").unwrap();
        m.execute("42").unwrap();
        m.execute("fold.").unwrap();
        m.execute("view.").unwrap();
        // Everything should be unfolded
        assert!(!m.stack[0].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
        assert!(!m.stack[1].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
    }

    #[test]
    fn fold_range() {
        let mut m = test_machine();
        m.execute("10 20 30 40 fold.0-1").unwrap();
        // Top two (40, 30) should be folded
        assert!(m.stack[2].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
        assert!(m.stack[3].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
        // Bottom two untouched
        assert!(!m.stack[0].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
        assert!(!m.stack[1].meta.get("folded").and_then(|v| v.as_bool()).unwrap_or(false));
    }

    #[test]
    fn fold_skips_list() {
        let mut m = test_machine();
        m.execute("[1 2 3] fold").unwrap();
        // List meta is an array — fold should skip it, not destroy data
        assert_eq!(m.stack[0].kind, "list");
        assert!(m.stack[0].meta.is_array());
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
