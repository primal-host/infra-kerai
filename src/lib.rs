pgrx::pg_module_magic!();

mod agents;
mod bootstrap;
mod consensus;
mod crdt;
mod functions;
mod identity;
mod marketplace;
mod parser;
mod peers;
mod perspectives;
mod query;
mod reconstruct;
mod schema;
mod swarm;
mod tasks;
mod workers;
mod zkp;

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

    // --- Plan 03: Reconstruction tests ---

    /// Helper: format source through prettyplease for canonical comparison.
    fn pretty(source: &str) -> String {
        let parsed = syn::parse_file(source).expect("test source should parse");
        prettyplease::unparse(&parsed)
    }

    /// Helper: parse source, then reconstruct and compare via prettyplease.
    fn assert_roundtrip(source: &str, filename: &str) {
        // Parse into nodes
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', '{}')",
            source.replace('\'', "''"),
            filename.replace('\'', "''"),
        ))
        .unwrap();

        // Get the file node ID
        let file_id = Spi::get_one::<pgrx::Uuid>(&format!(
            "SELECT id FROM kerai.nodes WHERE kind = 'file' AND content = '{}'",
            filename.replace('\'', "''"),
        ))
        .unwrap()
        .unwrap();

        // Reconstruct
        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_file('{}'::uuid)",
            file_id,
        ))
        .unwrap()
        .unwrap();

        let expected = pretty(source);
        assert_eq!(
            reconstructed.trim(),
            expected.trim(),
            "Round-trip mismatch for {}",
            filename
        );
    }

    #[pg_test]
    fn test_reconstruct_simple_fn() {
        assert_roundtrip("fn hello() { let x = 1; }", "recon_simple_fn.rs");
    }

    #[pg_test]
    fn test_reconstruct_roundtrip_struct() {
        assert_roundtrip(
            "#[derive(Debug, Clone)]\nstruct Point {\n    x: f64,\n    y: f64,\n}",
            "recon_struct.rs",
        );
    }

    #[pg_test]
    fn test_reconstruct_const_with_value() {
        assert_roundtrip("const MAX: u32 = 100;", "recon_const.rs");
    }

    #[pg_test]
    fn test_reconstruct_type_alias() {
        assert_roundtrip("type Pair = (i32, i32);", "recon_type_alias.rs");
    }

    #[pg_test]
    fn test_reconstruct_macro_with_args() {
        // Macros at top level — wrapped in nothing, just a macro invocation
        assert_roundtrip("fn f() { println!(\"hello {}\", 42); }", "recon_macro.rs");
    }

    #[pg_test]
    fn test_reconstruct_doc_comments() {
        assert_roundtrip(
            "/// This is documented\nfn documented() {}",
            "recon_doc.rs",
        );
    }

    #[pg_test]
    fn test_reconstruct_impl_block() {
        assert_roundtrip(
            "struct Foo;\nimpl Foo {\n    fn bar(&self) -> i32 { 42 }\n    const X: i32 = 1;\n    type Out = String;\n}",
            "recon_impl.rs",
        );
    }

    #[pg_test]
    #[should_panic(expected = "Failed to query node")]
    fn test_reconstruct_nonexistent_node() {
        Spi::get_one::<String>(
            "SELECT kerai.reconstruct_file('00000000-0000-0000-0000-000000000000'::uuid)",
        )
        .unwrap();
    }

    #[pg_test]
    #[should_panic(expected = "expected 'file'")]
    fn test_reconstruct_wrong_node_kind() {
        // Parse something first to get a non-file node
        Spi::run("SELECT kerai.parse_source('fn f() {}', 'recon_wrong_kind.rs')")
            .unwrap();

        let fn_id = Spi::get_one::<pgrx::Uuid>(
            "SELECT id FROM kerai.nodes WHERE kind = 'fn' AND content = 'f' LIMIT 1",
        )
        .unwrap()
        .unwrap();

        Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_file('{}'::uuid)",
            fn_id
        ))
        .unwrap();
    }

    #[pg_test]
    fn test_reconstruct_complex_roundtrip() {
        let source = "\
use std::collections::HashMap;

const VERSION: &str = \"1.0\";

fn compute(x: i32, y: i32) -> i32 {
    x + y
}

#[derive(Debug)]
struct Config {
    name: String,
    values: HashMap<String, i32>,
}

impl Config {
    fn new(name: String) -> Self {
        Config {
            name,
            values: HashMap::new(),
        }
    }
}";
        assert_roundtrip(source, "recon_complex.rs");
    }

    // --- Plan 04: CRDT operation tests ---

    #[pg_test]
    fn test_crdt_insert_node_via_apply_op() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"crdt_test_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["op_type"].as_str().unwrap(), "insert_node");
        assert!(obj.contains_key("node_id"));
        assert!(obj.contains_key("lamport_ts"));
        assert!(obj.contains_key("author_seq"));

        // Verify node exists
        let count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'fn' AND content = 'crdt_test_fn'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(count, 1, "Node should exist after insert_node op");
    }

    #[pg_test]
    fn test_crdt_update_content() {
        // Insert a node first
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"old_name\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let node_id = result.0["node_id"].as_str().unwrap().to_string();

        // Update content
        Spi::run(&format!(
            "SELECT kerai.apply_op('update_content', '{}'::uuid, '{{\"new_content\": \"new_name\"}}'::jsonb)",
            node_id,
        ))
        .unwrap();

        let content = Spi::get_one::<String>(&format!(
            "SELECT content FROM kerai.nodes WHERE id = '{}'::uuid",
            node_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(content, "new_name");
    }

    #[pg_test]
    fn test_crdt_update_metadata() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"meta_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let node_id = result.0["node_id"].as_str().unwrap().to_string();

        Spi::run(&format!(
            "SELECT kerai.apply_op('update_metadata', '{}'::uuid, '{{\"merge\": {{\"visibility\": \"public\"}}}}'::jsonb)",
            node_id,
        ))
        .unwrap();

        let meta = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT metadata FROM kerai.nodes WHERE id = '{}'::uuid",
            node_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(meta.0["visibility"].as_str().unwrap(), "public");
    }

    #[pg_test]
    fn test_crdt_move_node() {
        // Create parent
        let p = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"module\", \"content\": \"parent_mod\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let parent_id = p.0["node_id"].as_str().unwrap().to_string();

        // Create child
        let c = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"child_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let child_id = c.0["node_id"].as_str().unwrap().to_string();

        // Move child under parent at position 5
        Spi::run(&format!(
            "SELECT kerai.apply_op('move_node', '{}'::uuid, '{{\"new_parent_id\": \"{}\", \"new_position\": 5}}'::jsonb)",
            child_id, parent_id,
        ))
        .unwrap();

        let (pid, pos) = Spi::get_two::<String, i32>(&format!(
            "SELECT parent_id::text, position FROM kerai.nodes WHERE id = '{}'::uuid",
            child_id,
        ))
        .unwrap();
        assert_eq!(pid.unwrap(), parent_id);
        assert_eq!(pos.unwrap(), 5);
    }

    #[pg_test]
    fn test_crdt_delete_node() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"doomed_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let node_id = result.0["node_id"].as_str().unwrap().to_string();

        Spi::run(&format!(
            "SELECT kerai.apply_op('delete_node', '{}'::uuid, '{{\"cascade\": false}}'::jsonb)",
            node_id,
        ))
        .unwrap();

        let count = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE id = '{}'::uuid",
            node_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(count, 0, "Node should be deleted");
    }

    #[pg_test]
    fn test_crdt_delete_node_cascade() {
        // Create parent
        let p = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"module\", \"content\": \"cascade_parent\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let parent_id = p.0["node_id"].as_str().unwrap().to_string();

        // Create child with parent_id
        Spi::run(&format!(
            "SELECT kerai.apply_op('insert_node', NULL, '{{\"kind\": \"fn\", \"content\": \"cascade_child\", \"position\": 0, \"parent_id\": \"{}\"}}'::jsonb)",
            parent_id,
        ))
        .unwrap();

        // Cascade delete parent
        Spi::run(&format!(
            "SELECT kerai.apply_op('delete_node', '{}'::uuid, '{{\"cascade\": true}}'::jsonb)",
            parent_id,
        ))
        .unwrap();

        let count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE content IN ('cascade_parent', 'cascade_child')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(count, 0, "Parent and child should both be deleted");
    }

    #[pg_test]
    fn test_crdt_version_vector_increments() {
        // Two ops should produce author_seq >= 2
        Spi::run(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"vv1\", \"position\": 0}'::jsonb)",
        )
        .unwrap();
        Spi::run(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"vv2\", \"position\": 1}'::jsonb)",
        )
        .unwrap();

        let vv = Spi::get_one::<pgrx::JsonB>("SELECT kerai.version_vector()")
            .unwrap()
            .unwrap();
        let obj = vv.0.as_object().unwrap();
        // There should be at least one author with seq >= 2
        let max_seq = obj.values().filter_map(|v| v.as_i64()).max().unwrap_or(0);
        assert!(max_seq >= 2, "Version vector should show seq >= 2 after two ops");
    }

    #[pg_test]
    fn test_crdt_lamport_clock_increments() {
        let before = Spi::get_one::<i64>("SELECT kerai.lamport_clock()")
            .unwrap()
            .unwrap();

        Spi::run(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"lc_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap();

        let after = Spi::get_one::<i64>("SELECT kerai.lamport_clock()")
            .unwrap()
            .unwrap();
        assert!(after > before, "Lamport clock should increase after an op");
    }

    #[pg_test]
    #[should_panic(expected = "duplicate key value violates unique constraint")]
    fn test_crdt_idempotent_replay() {
        // Get current author fingerprint
        let fp = Spi::get_one::<String>(
            "SELECT key_fingerprint FROM kerai.instances WHERE is_self = true",
        )
        .unwrap()
        .unwrap();

        // Apply an op
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"replay_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let seq = result.0["author_seq"].as_i64().unwrap();
        let ts = result.0["lamport_ts"].as_i64().unwrap();

        // Manually try to insert a duplicate (author, author_seq) — should fail
        Spi::run(&format!(
            "INSERT INTO kerai.operations (instance_id, op_type, author, lamport_ts, author_seq, payload)
             SELECT id, 'insert_node', '{}', {}, {}, '{{}}'::jsonb FROM kerai.instances WHERE is_self = true",
            fp.replace('\'', "''"),
            ts + 1,
            seq,
        ))
        .unwrap();
    }

    #[pg_test]
    fn test_crdt_insert_and_delete_edge() {
        // Create two nodes
        let n1 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"edge_src\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let src_id = n1.0["node_id"].as_str().unwrap().to_string();

        let n2 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"edge_tgt\", \"position\": 1}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let tgt_id = n2.0["node_id"].as_str().unwrap().to_string();

        // Insert edge
        Spi::run(&format!(
            "SELECT kerai.apply_op('insert_edge', '{}'::uuid, '{{\"target_id\": \"{}\", \"relation\": \"calls\"}}'::jsonb)",
            src_id, tgt_id,
        ))
        .unwrap();

        let edge_count = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM kerai.edges WHERE source_id = '{}'::uuid AND target_id = '{}'::uuid AND relation = 'calls'",
            src_id, tgt_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(edge_count, 1, "Edge should exist");

        // Delete edge
        Spi::run(&format!(
            "SELECT kerai.apply_op('delete_edge', '{}'::uuid, '{{\"target_id\": \"{}\", \"relation\": \"calls\"}}'::jsonb)",
            src_id, tgt_id,
        ))
        .unwrap();

        let edge_count2 = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM kerai.edges WHERE source_id = '{}'::uuid AND target_id = '{}'::uuid AND relation = 'calls'",
            src_id, tgt_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(edge_count2, 0, "Edge should be deleted");
    }

    #[pg_test]
    fn test_crdt_signature_present() {
        Spi::run(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"sig_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap();

        let sig_len = Spi::get_one::<i32>(
            "SELECT octet_length(signature) FROM kerai.operations ORDER BY created_at DESC LIMIT 1",
        )
        .unwrap()
        .unwrap();
        assert_eq!(sig_len, 64, "Ed25519 signature should be 64 bytes");
    }

    #[pg_test]
    fn test_crdt_ops_since_returns_operations() {
        let fp = Spi::get_one::<String>(
            "SELECT key_fingerprint FROM kerai.instances WHERE is_self = true",
        )
        .unwrap()
        .unwrap();

        // Apply two ops
        Spi::run(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"ops_since_1\", \"position\": 0}'::jsonb)",
        )
        .unwrap();
        let r2 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"ops_since_2\", \"position\": 1}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let seq2 = r2.0["author_seq"].as_i64().unwrap();

        // Get ops since seq2-1 (should include at least the last op)
        let ops = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.ops_since('{}', {})",
            fp.replace('\'', "''"),
            seq2 - 1,
        ))
        .unwrap()
        .unwrap();
        let arr = ops.0.as_array().unwrap();
        assert!(!arr.is_empty(), "ops_since should return at least one op");
    }

    #[pg_test]
    #[should_panic(expected = "Unknown op_type")]
    fn test_crdt_invalid_op_type() {
        Spi::run(
            "SELECT kerai.apply_op('bogus_op', NULL, '{}'::jsonb)",
        )
        .unwrap();
    }

    #[pg_test]
    #[should_panic(expected = "requires a node_id")]
    fn test_crdt_update_without_node_id() {
        Spi::run(
            "SELECT kerai.apply_op('update_content', NULL, '{\"new_content\": \"x\"}'::jsonb)",
        )
        .unwrap();
    }

    // --- Plan 06: Peer sync tests ---

    /// Generate a test Ed25519 keypair. Returns (public_key_hex, fingerprint).
    fn generate_test_keypair() -> (String, String) {
        let mut rng = rand::rngs::OsRng;
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();
        let pk_hex: String = verifying_key.as_bytes().iter().map(|b| format!("{:02x}", b)).collect();
        let fp = crate::identity::fingerprint(&verifying_key);
        (pk_hex, fp)
    }

    #[pg_test]
    fn test_register_peer() {
        let (pk_hex, _fp) = generate_test_keypair();
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.register_peer('test-peer-1', '{}', 'https://peer1.example.com', NULL)",
            pk_hex,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["name"].as_str().unwrap(), "test-peer-1");
        assert!(obj["is_new"].as_bool().unwrap());
        assert!(obj.contains_key("key_fingerprint"));
        assert!(obj.contains_key("id"));
    }

    #[pg_test]
    fn test_register_peer_idempotent() {
        let (pk_hex, _) = generate_test_keypair();
        // First registration
        let r1 = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.register_peer('peer-idem', '{}', NULL, NULL)",
            pk_hex,
        ))
        .unwrap()
        .unwrap();
        assert!(r1.0["is_new"].as_bool().unwrap());

        // Second registration with updated endpoint
        let r2 = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.register_peer('peer-idem-updated', '{}', 'https://updated.example.com', NULL)",
            pk_hex,
        ))
        .unwrap()
        .unwrap();
        assert!(!r2.0["is_new"].as_bool().unwrap());
        assert_eq!(r2.0["endpoint"].as_str().unwrap(), "https://updated.example.com");
    }

    #[pg_test]
    fn test_list_peers_after_add() {
        let (pk_hex, _) = generate_test_keypair();
        Spi::run(&format!(
            "SELECT kerai.register_peer('list-test-peer', '{}', NULL, NULL)",
            pk_hex,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>("SELECT kerai.list_peers()")
            .unwrap()
            .unwrap();
        let arr = result.0.as_array().unwrap();
        let names: Vec<&str> = arr.iter().filter_map(|v| v["name"].as_str()).collect();
        assert!(names.contains(&"list-test-peer"), "Registered peer should appear in list");
    }

    #[pg_test]
    fn test_get_peer() {
        let (pk_hex, fp) = generate_test_keypair();
        Spi::run(&format!(
            "SELECT kerai.register_peer('get-test-peer', '{}', NULL, NULL)",
            pk_hex,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.get_peer('{}')",
            fp.replace('\'', "''"),
        ))
        .unwrap()
        .unwrap();
        assert_eq!(result.0["name"].as_str().unwrap(), "get-test-peer");
    }

    #[pg_test]
    fn test_remove_peer() {
        let (pk_hex, _) = generate_test_keypair();
        Spi::run(&format!(
            "SELECT kerai.register_peer('remove-me', '{}', NULL, NULL)",
            pk_hex,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.remove_peer('remove-me')",
        )
        .unwrap()
        .unwrap();
        assert!(result.0["removed"].as_bool().unwrap());
    }

    #[pg_test]
    #[should_panic(expected = "Cannot remove self instance")]
    fn test_cannot_remove_self() {
        let self_name = Spi::get_one::<String>(
            "SELECT name FROM kerai.instances WHERE is_self = true",
        )
        .unwrap()
        .unwrap();

        Spi::run(&format!(
            "SELECT kerai.remove_peer('{}')",
            self_name.replace('\'', "''"),
        ))
        .unwrap();
    }

    #[pg_test]
    fn test_self_public_key_hex() {
        let pk_hex = Spi::get_one::<String>("SELECT kerai.self_public_key_hex()")
            .unwrap()
            .unwrap();
        assert_eq!(pk_hex.len(), 64, "Hex-encoded 32-byte key should be 64 chars");
    }

    #[pg_test]
    fn test_ops_since_includes_public_key() {
        // Create an op first
        Spi::run(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"pk_test\", \"position\": 0}'::jsonb)",
        )
        .unwrap();

        let fp = Spi::get_one::<String>(
            "SELECT key_fingerprint FROM kerai.instances WHERE is_self = true",
        )
        .unwrap()
        .unwrap();

        let ops = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.ops_since('{}', 0)",
            fp.replace('\'', "''"),
        ))
        .unwrap()
        .unwrap();
        let arr = ops.0.as_array().unwrap();
        assert!(!arr.is_empty());
        // Each op should have a public_key field
        let first = &arr[0];
        assert!(first.get("public_key").is_some(), "ops_since should include public_key");
        let pk = first["public_key"].as_str().unwrap();
        assert_eq!(pk.len(), 64, "public_key should be 64 hex chars");
    }

    // --- Plan 07: Query / Navigation tests ---

    #[pg_test]
    fn test_find_by_content_pattern() {
        Spi::run("SELECT kerai.parse_source('fn hello_world() {} fn hello_there() {} fn goodbye() {}', 'find_pattern.rs')").unwrap();
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.find('%hello%', NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        let fn_matches: Vec<_> = arr.iter().filter(|v| v["kind"].as_str() == Some("fn")).collect();
        assert!(fn_matches.len() >= 2, "Should find at least 2 fn nodes matching '%hello%', got {}", fn_matches.len());
    }

    #[pg_test]
    fn test_find_with_kind_filter() {
        Spi::run("SELECT kerai.parse_source('struct Hello; fn Hello() {}', 'find_kind.rs')").unwrap();
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.find('Hello', 'struct', NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(!arr.is_empty(), "Should find at least one struct named Hello");
        for item in arr {
            assert_eq!(item["kind"].as_str().unwrap(), "struct", "All results should be struct kind");
        }
    }

    #[pg_test]
    fn test_find_with_limit() {
        Spi::run("SELECT kerai.parse_source('fn aaa() {} fn aab() {} fn aac() {}', 'find_limit.rs')").unwrap();
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.find('a%', NULL, 1)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(arr.len() <= 1, "Limit=1 should return at most 1 result, got {}", arr.len());
    }

    #[pg_test]
    fn test_find_no_matches() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.find('zzz_nonexistent_xyz_%', NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(arr.is_empty(), "Nonexistent pattern should return empty array");
    }

    #[pg_test]
    fn test_refs_finds_definitions_and_impls() {
        let source = "struct Config {} impl Config { fn new() -> Self { Config {} } }";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'refs_test.rs')",
            source.replace('\'', "''"),
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.refs('Config')",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["symbol"].as_str().unwrap(), "Config");

        let defs = obj["definitions"].as_array().unwrap();
        assert!(!defs.is_empty(), "Should find at least 1 definition of Config");

        let impls = obj["impls"].as_array().unwrap();
        assert!(!impls.is_empty(), "Should find at least 1 impl of Config");
    }

    #[pg_test]
    fn test_refs_nonexistent_symbol() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.refs('zzz_nonexistent_symbol_xyz')",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["definitions"].as_array().unwrap().is_empty());
        assert!(obj["references"].as_array().unwrap().is_empty());
        assert!(obj["impls"].as_array().unwrap().is_empty());
    }

    #[pg_test]
    fn test_tree_top_level() {
        Spi::run("SELECT kerai.parse_source('fn top_fn() {}', 'tree_top.rs')").unwrap();
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.tree(NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        let file_nodes: Vec<_> = arr.iter().filter(|v| v["kind"].as_str() == Some("file")).collect();
        assert!(!file_nodes.is_empty(), "Top-level tree should include file nodes");
    }

    #[pg_test]
    fn test_tree_with_path() {
        Spi::run("SELECT kerai.parse_source('fn nested() {}', 'tree_path.rs')").unwrap();
        // Get the file node's path
        let file_path = Spi::get_one::<String>(
            "SELECT path::text FROM kerai.nodes WHERE kind = 'file' AND content = 'tree_path.rs'",
        )
        .unwrap()
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.tree('{}')",
            sql_escape(&file_path),
        ))
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(!arr.is_empty(), "Tree with file path should find descendants");
    }

    #[pg_test]
    fn test_children_of_file_node() {
        Spi::run("SELECT kerai.parse_source('fn child_a() {} fn child_b() {}', 'children_test.rs')").unwrap();
        let file_id = Spi::get_one::<pgrx::Uuid>(
            "SELECT id FROM kerai.nodes WHERE kind = 'file' AND content = 'children_test.rs'",
        )
        .unwrap()
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.children('{}'::uuid)",
            file_id,
        ))
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        let fn_children: Vec<_> = arr.iter().filter(|v| v["kind"].as_str() == Some("fn")).collect();
        assert!(fn_children.len() >= 2, "File node should have at least 2 fn children, got {}", fn_children.len());
    }

    #[pg_test]
    fn test_ancestors_of_nested_node() {
        Spi::run("SELECT kerai.parse_source('fn outer() { let x = 1; }', 'ancestors_test.rs')").unwrap();
        // Find a stmt_local node (the `let x = 1;`)
        let local_id = Spi::get_one::<pgrx::Uuid>(
            "SELECT id FROM kerai.nodes WHERE kind = 'stmt_local' AND content = 'x' LIMIT 1",
        )
        .unwrap();

        if let Some(nid) = local_id {
            let result = Spi::get_one::<pgrx::JsonB>(&format!(
                "SELECT kerai.ancestors('{}'::uuid)",
                nid,
            ))
            .unwrap()
            .unwrap();
            let arr = result.0.as_array().unwrap();
            assert!(!arr.is_empty(), "Nested node should have ancestors");
            // Should eventually reach a file node
            let has_file = arr.iter().any(|v| v["kind"].as_str() == Some("file"));
            assert!(has_file, "Ancestors should include the file node");
        }
    }

    // --- Plan 08: Agent perspectives tests ---

    #[pg_test]
    fn test_register_agent() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.register_agent('test-llm', 'llm', 'claude-opus-4-6', NULL)",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["name"].as_str().unwrap(), "test-llm");
        assert_eq!(obj["kind"].as_str().unwrap(), "llm");
        assert!(obj["is_new"].as_bool().unwrap());
        assert!(obj.contains_key("id"));
    }

    #[pg_test]
    fn test_register_agent_idempotent() {
        Spi::run("SELECT kerai.register_agent('idem-agent', 'llm', 'model-a', NULL)")
            .unwrap();
        let r2 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.register_agent('idem-agent', 'tool', 'model-b', NULL)",
        )
        .unwrap()
        .unwrap();
        let obj = r2.0.as_object().unwrap();
        assert!(!obj["is_new"].as_bool().unwrap());
        assert_eq!(obj["kind"].as_str().unwrap(), "tool");
    }

    #[pg_test]
    fn test_list_agents() {
        Spi::run("SELECT kerai.register_agent('list-agent', 'human', NULL, NULL)")
            .unwrap();
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.list_agents(NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        let names: Vec<&str> = arr.iter().filter_map(|v| v["name"].as_str()).collect();
        assert!(names.contains(&"list-agent"));
    }

    #[pg_test]
    fn test_remove_agent() {
        Spi::run("SELECT kerai.register_agent('remove-agent', 'tool', NULL, NULL)")
            .unwrap();
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.remove_agent('remove-agent')",
        )
        .unwrap()
        .unwrap();
        assert!(result.0["removed"].as_bool().unwrap());
    }

    #[pg_test]
    fn test_set_perspective() {
        // Register agent and create a node
        Spi::run("SELECT kerai.register_agent('persp-agent', 'llm', NULL, NULL)")
            .unwrap();
        let node = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"persp_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let node_id = node.0["node_id"].as_str().unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.set_perspective('persp-agent', '{}'::uuid, 0.8, NULL, 'important function')",
            node_id,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["agent"].as_str().unwrap(), "persp-agent");
        assert_eq!(obj["weight"].as_f64().unwrap(), 0.8);

        // Verify stored
        let count = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM kerai.perspectives WHERE node_id = '{}'::uuid",
            node_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_set_perspective_with_context() {
        Spi::run("SELECT kerai.register_agent('ctx-agent', 'llm', NULL, NULL)")
            .unwrap();
        // Create two nodes — one as target, one as context
        let n1 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"ctx_target\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let n2 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"module\", \"content\": \"ctx_scope\", \"position\": 1}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let target_id = n1.0["node_id"].as_str().unwrap();
        let context_id = n2.0["node_id"].as_str().unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.set_perspective('ctx-agent', '{}'::uuid, 0.5, '{}'::uuid, NULL)",
            target_id, context_id,
        ))
        .unwrap()
        .unwrap();
        assert!(result.0["context_id"].as_str().is_some());
    }

    #[pg_test]
    fn test_delete_perspective() {
        Spi::run("SELECT kerai.register_agent('del-persp-agent', 'llm', NULL, NULL)")
            .unwrap();
        let node = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"del_persp_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let node_id = node.0["node_id"].as_str().unwrap();

        Spi::run(&format!(
            "SELECT kerai.set_perspective('del-persp-agent', '{}'::uuid, 0.9, NULL, NULL)",
            node_id,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.delete_perspective('del-persp-agent', '{}'::uuid, NULL)",
            node_id,
        ))
        .unwrap()
        .unwrap();
        assert!(result.0["deleted"].as_bool().unwrap());
    }

    #[pg_test]
    fn test_get_perspectives_with_filter() {
        Spi::run("SELECT kerai.register_agent('filter-agent', 'llm', NULL, NULL)")
            .unwrap();
        let n1 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"high_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let n2 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"low_fn\", \"position\": 1}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let high_id = n1.0["node_id"].as_str().unwrap();
        let low_id = n2.0["node_id"].as_str().unwrap();

        Spi::run(&format!(
            "SELECT kerai.set_perspective('filter-agent', '{}'::uuid, 0.9, NULL, NULL)",
            high_id,
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT kerai.set_perspective('filter-agent', '{}'::uuid, 0.2, NULL, NULL)",
            low_id,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.get_perspectives('filter-agent', NULL, 0.5)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert_eq!(arr.len(), 1, "Only the high-weight perspective should pass filter");
        assert_eq!(arr[0]["weight"].as_f64().unwrap(), 0.9);
    }

    #[pg_test]
    fn test_set_association() {
        Spi::run("SELECT kerai.register_agent('assoc-agent', 'llm', NULL, NULL)")
            .unwrap();
        let n1 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"assoc_src\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let n2 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"assoc_tgt\", \"position\": 1}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let src_id = n1.0["node_id"].as_str().unwrap();
        let tgt_id = n2.0["node_id"].as_str().unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.set_association('assoc-agent', '{}'::uuid, '{}'::uuid, 0.7, 'depends_on', 'tight coupling')",
            src_id, tgt_id,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["relation"].as_str().unwrap(), "depends_on");
        assert_eq!(obj["weight"].as_f64().unwrap(), 0.7);
    }

    #[pg_test]
    fn test_delete_association() {
        Spi::run("SELECT kerai.register_agent('del-assoc-agent', 'llm', NULL, NULL)")
            .unwrap();
        let n1 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"del_assoc_src\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let n2 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"del_assoc_tgt\", \"position\": 1}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let src_id = n1.0["node_id"].as_str().unwrap();
        let tgt_id = n2.0["node_id"].as_str().unwrap();

        Spi::run(&format!(
            "SELECT kerai.set_association('del-assoc-agent', '{}'::uuid, '{}'::uuid, 0.5, 'similar_to', NULL)",
            src_id, tgt_id,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.delete_association('del-assoc-agent', '{}'::uuid, '{}'::uuid, 'similar_to')",
            src_id, tgt_id,
        ))
        .unwrap()
        .unwrap();
        assert!(result.0["deleted"].as_bool().unwrap());
    }

    #[pg_test]
    fn test_consensus_multiple_agents() {
        // Register two agents
        Spi::run("SELECT kerai.register_agent('cons-agent-1', 'llm', NULL, NULL)")
            .unwrap();
        Spi::run("SELECT kerai.register_agent('cons-agent-2', 'llm', NULL, NULL)")
            .unwrap();

        // Create a node
        let node = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"consensus_fn\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let node_id = node.0["node_id"].as_str().unwrap();

        // Both agents rate the same node
        Spi::run(&format!(
            "SELECT kerai.set_perspective('cons-agent-1', '{}'::uuid, 0.8, NULL, NULL)",
            node_id,
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT kerai.set_perspective('cons-agent-2', '{}'::uuid, 0.6, NULL, NULL)",
            node_id,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.consensus(NULL, 2, NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(!arr.is_empty(), "Should find consensus with 2+ agents");
        let first = &arr[0];
        assert_eq!(first["agent_count"].as_i64().unwrap(), 2);
        let avg = first["avg_weight"].as_f64().unwrap();
        assert!((avg - 0.7).abs() < 0.001, "Average should be ~0.7, got {}", avg);
    }

    #[pg_test]
    fn test_perspective_diff() {
        Spi::run("SELECT kerai.register_agent('diff-agent-a', 'llm', NULL, NULL)")
            .unwrap();
        Spi::run("SELECT kerai.register_agent('diff-agent-b', 'llm', NULL, NULL)")
            .unwrap();

        // Create shared and unique nodes
        let shared = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"diff_shared\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let only_a_node = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"fn\", \"content\": \"diff_only_a\", \"position\": 1}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let shared_id = shared.0["node_id"].as_str().unwrap();
        let only_a_id = only_a_node.0["node_id"].as_str().unwrap();

        // Agent A rates both nodes with different weights
        Spi::run(&format!(
            "SELECT kerai.set_perspective('diff-agent-a', '{}'::uuid, 0.9, NULL, NULL)",
            shared_id,
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT kerai.set_perspective('diff-agent-a', '{}'::uuid, 0.5, NULL, NULL)",
            only_a_id,
        ))
        .unwrap();

        // Agent B rates shared node with different weight
        Spi::run(&format!(
            "SELECT kerai.set_perspective('diff-agent-b', '{}'::uuid, 0.3, NULL, NULL)",
            shared_id,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.perspective_diff('diff-agent-a', 'diff-agent-b', NULL)",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();

        let only_in_a = obj["only_in_a"].as_array().unwrap();
        assert!(!only_in_a.is_empty(), "Agent A should have unique perspectives");

        let disagreements = obj["disagreements"].as_array().unwrap();
        assert!(!disagreements.is_empty(), "Should have at least one disagreement on shared node");
        let diff = disagreements[0]["diff"].as_f64().unwrap();
        assert!((diff - 0.6).abs() < 0.001, "Diff should be ~0.6, got {}", diff);
    }

    // --- Plan 09: Swarm task tests ---

    #[pg_test]
    fn test_create_task() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_task('Fix bug #42', 'cargo test', NULL, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["description"].as_str().unwrap(), "Fix bug #42");
        assert_eq!(obj["success_command"].as_str().unwrap(), "cargo test");
        assert_eq!(obj["status"].as_str().unwrap(), "pending");
        assert!(obj.contains_key("id"));
    }

    #[pg_test]
    fn test_create_task_with_scope() {
        // Create a scope node first
        let node = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.apply_op('insert_node', NULL, '{\"kind\": \"module\", \"content\": \"scope_mod\", \"position\": 0}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let node_id = node.0["node_id"].as_str().unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.create_task('Scoped task', 'make test', '{}'::uuid, 100, 300)",
            node_id,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["status"].as_str().unwrap(), "pending");
        assert!(obj["scope_node_id"].as_str().is_some());
        assert_eq!(obj["budget_ops"].as_i64().unwrap(), 100);
        assert_eq!(obj["budget_seconds"].as_i64().unwrap(), 300);
    }

    #[pg_test]
    fn test_list_tasks() {
        Spi::run("SELECT kerai.create_task('Task A', 'cmd_a', NULL, NULL, NULL)")
            .unwrap();
        Spi::run("SELECT kerai.create_task('Task B', 'cmd_b', NULL, NULL, NULL)")
            .unwrap();

        // List all
        let all = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.list_tasks(NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = all.0.as_array().unwrap();
        assert!(arr.len() >= 2, "Should have at least 2 tasks");

        // List with filter
        let pending = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.list_tasks('pending')",
        )
        .unwrap()
        .unwrap();
        let parr = pending.0.as_array().unwrap();
        for t in parr {
            assert_eq!(t["status"].as_str().unwrap(), "pending");
        }
    }

    #[pg_test]
    fn test_update_task_status() {
        let task = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_task('Update me', 'test cmd', NULL, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let task_id = task.0["id"].as_str().unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.update_task_status('{}'::uuid, 'running')",
            task_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(result.0["status"].as_str().unwrap(), "running");
    }

    #[pg_test]
    #[should_panic(expected = "Invalid task status")]
    fn test_update_task_invalid_status() {
        let task = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_task('Bad status', 'cmd', NULL, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let task_id = task.0["id"].as_str().unwrap();

        Spi::run(&format!(
            "SELECT kerai.update_task_status('{}'::uuid, 'bogus')",
            task_id,
        ))
        .unwrap();
    }

    #[pg_test]
    fn test_launch_swarm() {
        let task = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_task('Swarm task', 'cargo test', NULL, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let task_id = task.0["id"].as_str().unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.launch_swarm('{}'::uuid, 3, 'llm', 'claude-opus-4-6')",
            task_id,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["status"].as_str().unwrap(), "running");
        assert_eq!(obj["agent_count"].as_i64().unwrap(), 3);
        assert!(obj["swarm_name"].as_str().unwrap().starts_with("swarm-"));

        // Verify swarm agent was registered
        let swarm_name = obj["swarm_name"].as_str().unwrap();
        let agent_exists = Spi::get_one::<bool>(&format!(
            "SELECT EXISTS(SELECT 1 FROM kerai.agents WHERE name = '{}')",
            sql_escape(swarm_name),
        ))
        .unwrap()
        .unwrap();
        assert!(agent_exists, "Swarm agent should be registered");
    }

    #[pg_test]
    fn test_stop_swarm() {
        let task = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_task('Stop me', 'cmd', NULL, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let task_id = task.0["id"].as_str().unwrap();

        Spi::run(&format!(
            "SELECT kerai.launch_swarm('{}'::uuid, 2, 'llm', NULL)",
            task_id,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.stop_swarm('{}'::uuid)",
            task_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(result.0["status"].as_str().unwrap(), "stopped");
    }

    #[pg_test]
    fn test_record_test_result() {
        let task = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_task('Result task', 'cmd', NULL, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let task_id = task.0["id"].as_str().unwrap();

        Spi::run("SELECT kerai.register_agent('result-agent', 'llm', NULL, NULL)")
            .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.record_test_result('{}'::uuid, 'result-agent', true, 'all tests pass', 150, 5)",
            task_id,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["passed"].as_bool().unwrap());
        assert_eq!(obj["duration_ms"].as_i64().unwrap(), 150);
        assert_eq!(obj["ops_count"].as_i64().unwrap(), 5);

        // Verify stored
        let count = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM kerai.test_results WHERE task_id = '{}'::uuid",
            task_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(count, 1);
    }

    #[pg_test]
    fn test_swarm_leaderboard() {
        let task = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_task('Leaderboard task', 'cmd', NULL, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let task_id = task.0["id"].as_str().unwrap();

        Spi::run("SELECT kerai.register_agent('lb-agent-1', 'llm', NULL, NULL)")
            .unwrap();
        Spi::run("SELECT kerai.register_agent('lb-agent-2', 'llm', NULL, NULL)")
            .unwrap();

        // Agent 1: 2 pass, 1 fail
        Spi::run(&format!("SELECT kerai.record_test_result('{}'::uuid, 'lb-agent-1', true, NULL, 100, NULL)", task_id)).unwrap();
        Spi::run(&format!("SELECT kerai.record_test_result('{}'::uuid, 'lb-agent-1', true, NULL, 120, NULL)", task_id)).unwrap();
        Spi::run(&format!("SELECT kerai.record_test_result('{}'::uuid, 'lb-agent-1', false, NULL, 200, NULL)", task_id)).unwrap();

        // Agent 2: 1 pass
        Spi::run(&format!("SELECT kerai.record_test_result('{}'::uuid, 'lb-agent-2', true, NULL, 80, NULL)", task_id)).unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.swarm_leaderboard('{}'::uuid)",
            task_id,
        ))
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert_eq!(arr.len(), 2, "Should have 2 agents on leaderboard");

        // Agent 2 should be first (100% pass rate)
        assert_eq!(arr[0]["agent_name"].as_str().unwrap(), "lb-agent-2");
        assert_eq!(arr[0]["pass_count"].as_i64().unwrap(), 1);
    }

    #[pg_test]
    fn test_swarm_progress() {
        let task = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_task('Progress task', 'cmd', NULL, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let task_id = task.0["id"].as_str().unwrap();

        Spi::run("SELECT kerai.register_agent('prog-agent', 'llm', NULL, NULL)")
            .unwrap();

        Spi::run(&format!("SELECT kerai.record_test_result('{}'::uuid, 'prog-agent', true, NULL, 50, NULL)", task_id)).unwrap();
        Spi::run(&format!("SELECT kerai.record_test_result('{}'::uuid, 'prog-agent', false, NULL, 60, NULL)", task_id)).unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.swarm_progress('{}'::uuid)",
            task_id,
        ))
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(!arr.is_empty(), "Should have at least one time bucket");
        let first = &arr[0];
        assert_eq!(first["total"].as_i64().unwrap(), 2);
        assert_eq!(first["passed"].as_i64().unwrap(), 1);
        assert_eq!(first["failed"].as_i64().unwrap(), 1);
    }

    #[pg_test]
    fn test_swarm_status_overview() {
        Spi::run("SELECT kerai.create_task('Status task 1', 'cmd1', NULL, NULL, NULL)")
            .unwrap();
        Spi::run("SELECT kerai.create_task('Status task 2', 'cmd2', NULL, NULL, NULL)")
            .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.swarm_status(NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(arr.len() >= 2, "Should show at least 2 tasks in overview");
    }

    // --- Plan 10: Marketplace tests ---

    /// Helper: create an attestation for the self instance. Returns attestation_id.
    fn create_test_attestation(scope: &str, claim_type: &str) -> String {
        Spi::get_one::<String>(&format!(
            "INSERT INTO kerai.attestations (instance_id, scope, claim_type, perspective_count, avg_weight)
             SELECT id, '{}'::ltree, '{}', 3, 0.75
             FROM kerai.instances WHERE is_self = true
             RETURNING id::text",
            sql_escape(scope),
            sql_escape(claim_type),
        ))
        .unwrap()
        .unwrap()
    }

    #[pg_test]
    fn test_create_auction() {
        let att_id = create_test_attestation("pkg.auth", "expertise");
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 80000, 1000, 3600, 0, 1, 24)",
            att_id,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["starting_price"].as_i64().unwrap(), 80000);
        assert_eq!(obj["current_price"].as_i64().unwrap(), 80000);
        assert_eq!(obj["status"].as_str().unwrap(), "active");
    }

    #[pg_test]
    #[should_panic(expected = "active auction already exists")]
    fn test_create_auction_duplicate() {
        let att_id = create_test_attestation("pkg.dup", "expertise");
        Spi::run(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 50000, 500, 60, 0, 1, 24)",
            att_id,
        ))
        .unwrap();
        // Second auction on same attestation should fail
        Spi::run(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 50000, 500, 60, 0, 1, 24)",
            att_id,
        ))
        .unwrap();
    }

    #[pg_test]
    fn test_place_bid() {
        let att_id = create_test_attestation("pkg.bid", "state_transition");
        let auction = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 50000, 1000, 60, 0, 1, 24)",
            att_id,
        ))
        .unwrap()
        .unwrap();
        let auction_id = auction.0["id"].as_str().unwrap();

        let bid = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.place_bid('{}'::uuid, 40000)",
            auction_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(bid.0["max_price"].as_i64().unwrap(), 40000);
        assert!(bid.0.as_object().unwrap().contains_key("id"));
    }

    #[pg_test]
    fn test_tick_auction_price_decrement() {
        let att_id = create_test_attestation("pkg.tick", "expertise");
        let auction = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 10000, 2000, 60, 0, 1, 24)",
            att_id,
        ))
        .unwrap()
        .unwrap();
        let auction_id = auction.0["id"].as_str().unwrap();

        let tick = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.tick_auction('{}'::uuid)",
            auction_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(tick.0["current_price"].as_i64().unwrap(), 8000);
        assert_eq!(tick.0["action"].as_str().unwrap(), "price_decremented");
    }

    #[pg_test]
    fn test_tick_auction_floor_hit() {
        let att_id = create_test_attestation("pkg.floor", "expertise");
        let auction = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 3000, 5000, 60, 0, 1, 24)",
            att_id,
        ))
        .unwrap()
        .unwrap();
        let auction_id = auction.0["id"].as_str().unwrap();

        // Decrement 5000 from 3000 should hit floor
        let tick = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.tick_auction('{}'::uuid)",
            auction_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(tick.0["action"].as_str().unwrap(), "open_sourced");
        assert_eq!(tick.0["reason"].as_str().unwrap(), "floor_price_hit");
    }

    #[pg_test]
    fn test_tick_auction_settlement_ready() {
        let att_id = create_test_attestation("pkg.settle_ready", "expertise");
        let auction = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 50000, 1000, 60, 0, 1, 24)",
            att_id,
        ))
        .unwrap()
        .unwrap();
        let auction_id = auction.0["id"].as_str().unwrap();

        // Place a bid high enough for the decremented price
        Spi::run(&format!(
            "SELECT kerai.place_bid('{}'::uuid, 49000)",
            auction_id,
        ))
        .unwrap();

        let tick = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.tick_auction('{}'::uuid)",
            auction_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(tick.0["action"].as_str().unwrap(), "settlement_ready");
        assert!(tick.0["qualifying_bidders"].as_i64().unwrap() >= 1);
    }

    #[pg_test]
    fn test_settle_auction() {
        let att_id = create_test_attestation("pkg.settle", "expertise");
        let auction = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 10000, 1000, 60, 0, 1, 24)",
            att_id,
        ))
        .unwrap()
        .unwrap();
        let auction_id = auction.0["id"].as_str().unwrap();

        // Place a bid
        Spi::run(&format!(
            "SELECT kerai.place_bid('{}'::uuid, 10000)",
            auction_id,
        ))
        .unwrap();

        // Settle at current price (10000)
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.settle_auction('{}'::uuid)",
            auction_id,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["status"].as_str().unwrap(), "settled");
        assert_eq!(obj["settled_price"].as_i64().unwrap(), 10000);
        assert_eq!(obj["bidder_count"].as_i64().unwrap(), 1);
        assert_eq!(obj["total_revenue"].as_i64().unwrap(), 10000);
    }

    #[pg_test]
    fn test_open_source_auction() {
        let att_id = create_test_attestation("pkg.opensource", "expertise");
        let auction = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 5000, 500, 60, 0, 1, 0)",
            att_id,
        ))
        .unwrap()
        .unwrap();
        let auction_id = auction.0["id"].as_str().unwrap();

        // Place bid and settle
        Spi::run(&format!(
            "SELECT kerai.place_bid('{}'::uuid, 5000)",
            auction_id,
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT kerai.settle_auction('{}'::uuid)",
            auction_id,
        ))
        .unwrap();

        // Open-source
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.open_source_auction('{}'::uuid)",
            auction_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(result.0["status"].as_str().unwrap(), "open_sourced");
    }

    #[pg_test]
    fn test_market_browse() {
        let att_id = create_test_attestation("pkg.browse", "expertise");
        Spi::run(&format!(
            "SELECT kerai.create_auction('{}'::uuid, 20000, 500, 60, 0, 1, 24)",
            att_id,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.market_browse(NULL, NULL, 'active')",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(!arr.is_empty(), "Should find at least one active auction");
    }

    #[pg_test]
    fn test_market_stats() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.market_stats()",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj.contains_key("active_auctions"));
        assert!(obj.contains_key("settled_auctions"));
        assert!(obj.contains_key("open_sourced"));
        assert!(obj.contains_key("total_bids"));
        assert!(obj.contains_key("total_settlement_value"));
        assert!(obj.contains_key("avg_settlement_price"));
    }

    #[pg_test]
    fn test_generate_and_verify_proof() {
        let att_id = create_test_attestation("pkg.zkp", "state_transition");

        // Generate proof
        let proof = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.generate_proof('{}'::uuid)",
            att_id,
        ))
        .unwrap()
        .unwrap();
        let obj = proof.0.as_object().unwrap();
        assert_eq!(obj["proof_type"].as_str().unwrap(), "sha256_commitment");
        let proof_hex = obj["proof_hex"].as_str().unwrap();
        assert_eq!(proof_hex.len(), 64, "SHA-256 hex should be 64 chars");

        // Verify proof using stored proof_data
        let verify = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.verify_proof('{}'::uuid,
                (SELECT proof_data FROM kerai.attestations WHERE id = '{}'::uuid))",
            att_id, att_id,
        ))
        .unwrap()
        .unwrap();
        assert!(verify.0["valid"].as_bool().unwrap(), "Proof should verify");
    }

    #[pg_test]
    fn test_verify_proof_invalid() {
        let att_id = create_test_attestation("pkg.bad_proof", "expertise");
        Spi::run(&format!(
            "SELECT kerai.generate_proof('{}'::uuid)",
            att_id,
        ))
        .unwrap();

        // Verify with wrong proof data
        let verify = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.verify_proof('{}'::uuid, '\\xdeadbeef'::bytea)",
            att_id,
        ))
        .unwrap()
        .unwrap();
        assert!(!verify.0["valid"].as_bool().unwrap(), "Invalid proof should fail");
    }

    #[pg_test]
    fn test_market_balance() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.market_balance()",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj.contains_key("earnings"));
        assert!(obj.contains_key("spending"));
        assert!(obj.contains_key("net"));
        assert!(obj.contains_key("active_auctions"));
        assert!(obj.contains_key("active_bids"));
    }

    // --- Plan 12: Markdown parser tests ---

    #[pg_test]
    fn test_parse_markdown_headings() {
        let source = "# Title\n\n## Section One\n\nParagraph under section one.\n\n## Section Two\n\n### Subsection\n\nDeep content.\n";
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.parse_markdown('{}', 'headings.md')",
            sql_escape(source),
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["nodes"].as_u64().unwrap() > 0, "Should have parsed nodes");

        // Verify heading hierarchy: H2 should be child of H1
        let h1_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes WHERE kind = 'heading' AND content = 'Title'",
        )
        .unwrap()
        .unwrap();

        let h2_parent = Spi::get_one::<String>(
            "SELECT parent_id::text FROM kerai.nodes WHERE kind = 'heading' AND content = 'Section One'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(h2_parent, h1_id, "H2 should be child of H1");

        // H3 should be child of H2 (Section Two)
        let h2_two_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes WHERE kind = 'heading' AND content = 'Section Two'",
        )
        .unwrap()
        .unwrap();

        let h3_parent = Spi::get_one::<String>(
            "SELECT parent_id::text FROM kerai.nodes WHERE kind = 'heading' AND content = 'Subsection'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(h3_parent, h2_two_id, "H3 should be child of its preceding H2");
    }

    #[pg_test]
    fn test_parse_markdown_paragraphs() {
        let source = "# Main\n\nFirst paragraph.\n\nSecond paragraph.\n";
        Spi::run(&format!(
            "SELECT kerai.parse_markdown('{}', 'paragraphs.md')",
            sql_escape(source),
        ))
        .unwrap();

        let heading_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes WHERE kind = 'heading' AND content = 'Main'",
        )
        .unwrap()
        .unwrap();

        // Paragraphs should be children of the heading
        let para_count = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'paragraph' AND parent_id = '{}'::uuid",
            heading_id,
        ))
        .unwrap()
        .unwrap();
        assert!(para_count >= 2, "Should have at least 2 paragraphs under heading, got {}", para_count);
    }

    #[pg_test]
    fn test_parse_markdown_code_block() {
        let source = "# Code\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n";
        Spi::run(&format!(
            "SELECT kerai.parse_markdown('{}', 'codeblock.md')",
            sql_escape(source),
        ))
        .unwrap();

        let lang = Spi::get_one::<pgrx::JsonB>(
            "SELECT metadata FROM kerai.nodes WHERE kind = 'code_block' LIMIT 1",
        )
        .unwrap()
        .unwrap();
        assert_eq!(lang.0["language"].as_str().unwrap(), "rust", "Code block should preserve language metadata");
    }

    #[pg_test]
    fn test_parse_markdown_links() {
        let source = "# Links\n\n[Example](https://example.com) and [local](other.md).\n";
        Spi::run(&format!(
            "SELECT kerai.parse_markdown('{}', 'links.md')",
            sql_escape(source),
        ))
        .unwrap();

        let link_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'link'",
        )
        .unwrap()
        .unwrap();
        assert!(link_count >= 2, "Should have at least 2 link nodes, got {}", link_count);

        // Check URL metadata
        let meta = Spi::get_one::<pgrx::JsonB>(
            "SELECT metadata FROM kerai.nodes WHERE kind = 'link' AND content LIKE '%Example%' LIMIT 1",
        )
        .unwrap()
        .unwrap();
        assert_eq!(meta.0["url"].as_str().unwrap(), "https://example.com");
    }

    #[pg_test]
    fn test_parse_markdown_table() {
        let source = "# Tables\n\n| Name | Value |\n| --- | --- |\n| foo | 42 |\n| bar | 99 |\n";
        Spi::run(&format!(
            "SELECT kerai.parse_markdown('{}', 'table.md')",
            sql_escape(source),
        ))
        .unwrap();

        let table_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'table'",
        )
        .unwrap()
        .unwrap();
        assert!(table_count >= 1, "Should have at least 1 table node");

        let cell_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'table_cell'",
        )
        .unwrap()
        .unwrap();
        assert!(cell_count >= 4, "Should have at least 4 table cells (2 cols x 2+ rows), got {}", cell_count);
    }

    #[pg_test]
    fn test_parse_markdown_roundtrip() {
        let source = "# Hello World\n\nThis is a paragraph.\n\n## Details\n\n- Item one\n- Item two\n";
        Spi::run(&format!(
            "SELECT kerai.parse_markdown('{}', 'roundtrip.md')",
            sql_escape(source),
        ))
        .unwrap();

        let doc_id = Spi::get_one::<pgrx::Uuid>(
            "SELECT id FROM kerai.nodes WHERE kind = 'document' AND content = 'roundtrip.md'",
        )
        .unwrap()
        .unwrap();

        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_markdown('{}'::uuid)",
            doc_id,
        ))
        .unwrap()
        .unwrap();

        // Verify key content is preserved
        assert!(reconstructed.contains("# Hello World"), "Should contain H1");
        assert!(reconstructed.contains("This is a paragraph"), "Should contain paragraph text");
        assert!(reconstructed.contains("## Details"), "Should contain H2");
        assert!(reconstructed.contains("Item one"), "Should contain list items");
    }

    #[pg_test]
    fn test_parse_markdown_idempotent() {
        let source = "# Idempotent\n\nSame content.\n";
        Spi::run(&format!(
            "SELECT kerai.parse_markdown('{}', 'idempotent.md')",
            sql_escape(source),
        ))
        .unwrap();
        let count1 = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'document' AND content = 'idempotent.md'",
        )
        .unwrap()
        .unwrap();

        // Parse again — should delete and re-insert
        Spi::run(&format!(
            "SELECT kerai.parse_markdown('{}', 'idempotent.md')",
            sql_escape(source),
        ))
        .unwrap();
        let count2 = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'document' AND content = 'idempotent.md'",
        )
        .unwrap()
        .unwrap();

        assert_eq!(count1, count2, "Idempotent parse should not duplicate document nodes");
        assert_eq!(count1, 1, "Should have exactly one document node");
    }

    // --- Plan 12: FTS search tests ---

    #[pg_test]
    fn test_search_fts_basic() {
        Spi::run(
            "SELECT kerai.parse_source('fn calculate_total() { let sum = 0; }', 'fts_basic.rs')",
        )
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.search('calculate', NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(!arr.is_empty(), "FTS should find nodes matching 'calculate'");
    }

    #[pg_test]
    fn test_search_fts_with_kind_filter() {
        Spi::run(
            "SELECT kerai.parse_source('struct SearchTarget { value: i32 }', 'fts_kind.rs')",
        )
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.search('SearchTarget', 'struct', NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        for item in arr {
            assert_eq!(item["kind"].as_str().unwrap(), "struct");
        }
    }

    #[pg_test]
    fn test_search_fts_no_matches() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.search('xyzzy_nonexistent_term_zzz', NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(arr.is_empty(), "FTS should return empty for non-matching terms");
    }

    #[pg_test]
    fn test_context_search_without_agents() {
        Spi::run(
            "SELECT kerai.parse_source('fn context_target() {}', 'ctx_search.rs')",
        )
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.context_search('context_target', NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(!arr.is_empty(), "context_search without agents should still return FTS results");
    }

    /// sql_escape helper for tests
    fn sql_escape(s: &str) -> String {
        s.replace('\'', "''")
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
