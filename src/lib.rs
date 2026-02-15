pgrx::pg_module_magic!();

mod bootstrap;
mod functions;
mod identity;
mod parser;
mod schema;
mod workers;

#[pgrx::pg_guard]
pub extern "C-unwind" fn _PG_init() {
    workers::register_workers();
}

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    #[pg_test]
    fn test_self_instance_exists() {
        let exists = Spi::get_one::<bool>(
            "SELECT EXISTS(SELECT 1 FROM kerai.instances WHERE is_self = true)",
        )
        .unwrap()
        .unwrap();
        assert!(exists, "Self instance should exist after bootstrap");
    }

    #[pg_test]
    fn test_self_instance_has_public_key() {
        let has_key = Spi::get_one::<bool>(
            "SELECT octet_length(public_key) = 32 FROM kerai.instances WHERE is_self = true",
        )
        .unwrap()
        .unwrap();
        assert!(has_key, "Self instance should have a 32-byte Ed25519 public key");
    }

    #[pg_test]
    fn test_self_instance_has_fingerprint() {
        let fp = Spi::get_one::<String>(
            "SELECT key_fingerprint FROM kerai.instances WHERE is_self = true",
        )
        .unwrap()
        .unwrap();
        assert!(!fp.is_empty(), "Fingerprint should not be empty");
        assert!(fp.ends_with('='), "Fingerprint should be base64-encoded");
    }

    #[pg_test]
    fn test_self_wallet_exists() {
        let exists = Spi::get_one::<bool>(
            "SELECT EXISTS(
                SELECT 1 FROM kerai.wallets w
                JOIN kerai.instances i ON w.instance_id = i.id
                WHERE i.is_self = true AND w.wallet_type = 'instance'
            )",
        )
        .unwrap()
        .unwrap();
        assert!(exists, "Self wallet should exist and be linked to self instance");
    }

    #[pg_test]
    fn test_wallet_balance_zero() {
        let balance = Spi::get_one::<i64>("SELECT kerai.wallet_balance()")
            .unwrap()
            .unwrap();
        assert_eq!(balance, 0, "Initial wallet balance should be 0");
    }

    #[pg_test]
    fn test_status_returns_json() {
        let status = Spi::get_one::<pgrx::JsonB>("SELECT kerai.status()")
            .unwrap()
            .unwrap();
        let obj = status.0.as_object().expect("Status should be a JSON object");
        assert!(obj.contains_key("instance_id"));
        assert!(obj.contains_key("name"));
        assert!(obj.contains_key("fingerprint"));
        assert!(obj.contains_key("peer_count"));
        assert!(obj.contains_key("node_count"));
        assert!(obj.contains_key("version_count"));
        assert_eq!(obj.get("version").unwrap(), "0.1.0");
    }

    #[pg_test]
    fn test_insert_nodes_with_ltree() {
        // Insert a crate node
        Spi::run(
            "INSERT INTO kerai.nodes (instance_id, kind, language, content, position, path)
             SELECT id, 'crate', 'rust', 'test_crate', 0, 'test_crate'::ltree
             FROM kerai.instances WHERE is_self = true",
        )
        .unwrap();

        // Insert a child module node
        Spi::run(
            "INSERT INTO kerai.nodes (instance_id, kind, language, content, parent_id, position, path)
             SELECT i.id, 'module', 'rust', 'test_mod', n.id, 0, 'test_crate.test_mod'::ltree
             FROM kerai.instances i, kerai.nodes n
             WHERE i.is_self = true AND n.content = 'test_crate'",
        )
        .unwrap();

        // Query with ltree descendant operator
        let count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE path <@ 'test_crate'::ltree",
        )
        .unwrap()
        .unwrap();
        assert_eq!(count, 2, "Should find 2 nodes under test_crate path");
    }

    #[pg_test]
    fn test_insert_edges() {
        Spi::run(
            "INSERT INTO kerai.nodes (instance_id, kind, content, position)
             SELECT id, 'fn', 'source_fn', 0 FROM kerai.instances WHERE is_self = true",
        )
        .unwrap();
        Spi::run(
            "INSERT INTO kerai.nodes (instance_id, kind, content, position)
             SELECT id, 'fn', 'target_fn', 1 FROM kerai.instances WHERE is_self = true",
        )
        .unwrap();

        Spi::run(
            "INSERT INTO kerai.edges (source_id, target_id, relation)
             SELECT n1.id, n2.id, 'calls'
             FROM kerai.nodes n1, kerai.nodes n2
             WHERE n1.content = 'source_fn' AND n2.content = 'target_fn'",
        )
        .unwrap();

        let rel = Spi::get_one::<String>(
            "SELECT relation FROM kerai.edges LIMIT 1",
        )
        .unwrap()
        .unwrap();
        assert_eq!(rel, "calls");
    }

    #[pg_test]
    fn test_insert_version() {
        Spi::run(
            "INSERT INTO kerai.nodes (instance_id, kind, content, position)
             SELECT id, 'fn', 'versioned_fn', 0 FROM kerai.instances WHERE is_self = true",
        )
        .unwrap();

        Spi::run(
            "INSERT INTO kerai.versions (node_id, instance_id, operation, new_content, author, timestamp)
             SELECT n.id, i.id, 'create', 'fn versioned_fn() {}', 'test_author', 1
             FROM kerai.nodes n, kerai.instances i
             WHERE n.content = 'versioned_fn' AND i.is_self = true",
        )
        .unwrap();

        let count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.versions WHERE author = 'test_author'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_ledger_balance_calculation() {
        // Get self wallet ID
        let wallet_id = Spi::get_one::<String>(
            "SELECT w.id::text FROM kerai.wallets w
             JOIN kerai.instances i ON w.instance_id = i.id
             WHERE i.is_self = true",
        )
        .unwrap()
        .unwrap();

        // Create a mint wallet for testing
        Spi::run(
            "INSERT INTO kerai.wallets (public_key, key_fingerprint, wallet_type, label)
             VALUES ('\\xdeadbeef'::bytea, 'mint-fp', 'system', 'Mint')",
        )
        .unwrap();

        // Mint 100 tokens to self wallet
        Spi::run(&format!(
            "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, timestamp)
             SELECT m.id, '{}'::uuid, 100, 'mint', 1
             FROM kerai.wallets m WHERE m.wallet_type = 'system'",
            wallet_id
        ))
        .unwrap();

        let balance = Spi::get_one::<i64>("SELECT kerai.wallet_balance()")
            .unwrap()
            .unwrap();
        assert_eq!(balance, 100, "Balance should be 100 after minting");
    }

    #[pg_test]
    #[should_panic(expected = "duplicate key value violates unique constraint")]
    fn test_unique_self_instance_constraint() {
        // Try inserting a second self instance — should fail with unique violation
        Spi::run(
            "INSERT INTO kerai.instances (name, public_key, key_fingerprint, is_self)
             VALUES ('fake', '\\xdeadbeef', 'fake-fp-unique-test', true)",
        )
        .unwrap();
    }

    #[pg_test]
    fn test_bootstrap_idempotent() {
        let result = Spi::get_one::<String>("SELECT kerai.bootstrap_instance()")
            .unwrap()
            .unwrap();
        assert_eq!(result, "already_bootstrapped", "Second bootstrap should be a no-op");
    }

    #[pg_test]
    fn test_parse_source_simple_fn() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.parse_source('fn hello() { let x = 1; }', 'test_simple.rs')",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        let node_count = obj["nodes"].as_u64().unwrap();
        assert!(node_count > 0, "Should have parsed nodes");

        // Verify function node exists
        let fn_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'fn' AND content = 'hello'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(fn_count, 1, "Should have one fn node named 'hello'");
    }

    #[pg_test]
    fn test_parse_source_struct_with_fields() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.parse_source('struct Point { x: f64, y: f64 }', 'test_struct.rs')",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["nodes"].as_u64().unwrap() > 0);

        let field_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'field' AND content IN ('x', 'y')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(field_count, 2, "Should have two field nodes");
    }

    #[pg_test]
    fn test_parse_source_impl_block() {
        let source = "struct Foo;
impl Foo {
    fn bar(&self) -> i32 { 42 }
}";
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.parse_source('{}', 'test_impl.rs')",
            source.replace('\'', "''")
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["nodes"].as_u64().unwrap() > 0);

        let impl_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'impl'",
        )
        .unwrap()
        .unwrap();
        assert!(impl_count >= 1, "Should have at least one impl node");

        let method_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'fn' AND content = 'bar'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(method_count, 1, "Should have method 'bar'");
    }

    #[pg_test]
    fn test_parse_source_returns_json_stats() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.parse_source('fn f() {}', 'test_stats.rs')",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj.contains_key("file"));
        assert!(obj.contains_key("nodes"));
        assert!(obj.contains_key("edges"));
        assert!(obj.contains_key("elapsed_ms"));
    }

    #[pg_test]
    fn test_parse_source_preserves_doc_comments() {
        let source = "/// This is a doc comment\nfn documented() {}";
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.parse_source('{}', 'test_doc.rs')",
            source.replace('\'', "''")
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["nodes"].as_u64().unwrap() > 0);

        let doc_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'doc_comment'",
        )
        .unwrap()
        .unwrap();
        assert!(doc_count >= 1, "Should have at least one doc_comment node");
    }

    #[pg_test]
    fn test_parse_source_macro_call() {
        let source = "fn main() { println!(\"hello\"); }";
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.parse_source('{}', 'test_macro.rs')",
            source.replace('\'', "''")
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["nodes"].as_u64().unwrap() > 0);

        let macro_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'macro_call'",
        )
        .unwrap()
        .unwrap();
        assert!(macro_count >= 1, "Should have at least one macro_call node");
    }

    #[pg_test]
    fn test_parse_source_expressions() {
        let source = "fn f() { if true { 1 } else { 2 }; match 1 { 0 => 0, _ => 1 }; }";
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.parse_source('{}', 'test_expr.rs')",
            source.replace('\'', "''")
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["nodes"].as_u64().unwrap() > 0);

        let if_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'expr_if'",
        )
        .unwrap()
        .unwrap();
        assert!(if_count >= 1, "Should have at least one expr_if node");

        let match_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'expr_match'",
        )
        .unwrap()
        .unwrap();
        assert!(match_count >= 1, "Should have at least one expr_match node");
    }

    #[pg_test]
    fn test_parse_source_idempotent() {
        Spi::run(
            "SELECT kerai.parse_source('fn dup() {}', 'test_idempotent.rs')",
        )
        .unwrap();
        let count1 = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'fn' AND content = 'dup'",
        )
        .unwrap()
        .unwrap();

        // Parse again — should delete and re-insert
        Spi::run(
            "SELECT kerai.parse_source('fn dup() {}', 'test_idempotent.rs')",
        )
        .unwrap();
        let count2 = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'fn' AND content = 'dup'",
        )
        .unwrap()
        .unwrap();

        assert_eq!(count1, count2, "Idempotent parse should not duplicate nodes");
    }

    #[pg_test]
    fn test_stub_find() {
        let result = Spi::get_one::<String>("SELECT kerai.find('test')")
            .unwrap()
            .unwrap();
        assert!(result.starts_with("STUB:"));
    }

    #[pg_test]
    fn test_stub_version_vector() {
        let result = Spi::get_one::<String>("SELECT kerai.version_vector()")
            .unwrap()
            .unwrap();
        assert!(result.starts_with("STUB:"));
    }
}

#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {}

    #[must_use]
    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}
