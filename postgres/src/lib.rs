pgrx::pg_module_magic!();

mod agents;
mod bootstrap;
mod bounties;
mod consensus;
mod crawler;
mod crdt;
mod currency;
mod economy;
mod functions;
mod identity;
mod marketplace;
mod microgpt;
pub(crate) mod parser;
mod peers;
mod repo;
mod perspectives;
mod query;
mod reconstruct;
mod schema;
pub mod sql;
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
            "#[derive(Clone, Debug)]\nstruct Point {\n    x: f64,\n    y: f64,\n}",
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

    // --- Plan 11: Economy tests ---

    /// Helper: get self wallet ID.
    fn get_self_wallet_id() -> String {
        Spi::get_one::<String>(
            "SELECT w.id::text FROM kerai.wallets w
             JOIN kerai.instances i ON w.instance_id = i.id
             WHERE i.is_self = true AND w.wallet_type = 'instance'",
        )
        .unwrap()
        .unwrap()
    }

    /// Helper: mint Koi to the self wallet and return the wallet ID.
    fn mint_to_self(amount: i64) -> String {
        let wallet_id = get_self_wallet_id();
        Spi::run(&format!(
            "SELECT kerai.mint_koi('{}'::uuid, {}, 'test mint', NULL, NULL)",
            wallet_id, amount,
        ))
        .unwrap();
        wallet_id
    }

    #[pg_test]
    fn test_create_wallet_human() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_wallet('human', 'Alice')",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["wallet_type"].as_str().unwrap(), "human");
        assert_eq!(obj["label"].as_str().unwrap(), "Alice");
        assert!(obj.contains_key("id"));
        assert!(obj.contains_key("key_fingerprint"));
    }

    #[pg_test]
    #[should_panic(expected = "Invalid wallet type")]
    fn test_create_wallet_invalid_type() {
        Spi::run("SELECT kerai.create_wallet('instance', NULL)")
            .unwrap();
    }

    #[pg_test]
    fn test_list_wallets() {
        // Create a human wallet
        Spi::run("SELECT kerai.create_wallet('human', 'List Test')")
            .unwrap();

        // List all
        let all = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.list_wallets(NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = all.0.as_array().unwrap();
        // Should have at least the bootstrap instance wallet + the new one
        assert!(arr.len() >= 2, "Should have at least 2 wallets, got {}", arr.len());

        // List filtered
        let humans = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.list_wallets('human')",
        )
        .unwrap()
        .unwrap();
        let harr = humans.0.as_array().unwrap();
        for w in harr {
            assert_eq!(w["wallet_type"].as_str().unwrap(), "human");
        }
    }

    #[pg_test]
    fn test_mint_koi() {
        let wallet_id = get_self_wallet_id();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mint_koi('{}'::uuid, 500, 'test reward', NULL, NULL)",
            wallet_id,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["amount"].as_i64().unwrap(), 500);
        assert_eq!(obj["reason"].as_str().unwrap(), "test reward");

        // Verify balance increased
        let bal = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.get_wallet_balance('{}'::uuid)",
            wallet_id,
        ))
        .unwrap()
        .unwrap();
        assert!(bal.0["balance"].as_i64().unwrap() >= 500);
    }

    #[pg_test]
    fn test_transfer_koi() {
        // Mint to self
        let self_wallet = mint_to_self(1000);

        // Create a human wallet
        let human = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_wallet('human', 'Transfer Target')",
        )
        .unwrap()
        .unwrap();
        let human_id = human.0["id"].as_str().unwrap().to_string();

        // Transfer 300 Koi
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.transfer_koi('{}'::uuid, '{}'::uuid, 300, 'payment')",
            self_wallet, human_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(result.0["amount"].as_i64().unwrap(), 300);

        // Verify recipient balance
        let bal = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.get_wallet_balance('{}'::uuid)",
            human_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(bal.0["balance"].as_i64().unwrap(), 300);
    }

    #[pg_test]
    #[should_panic(expected = "Insufficient balance")]
    fn test_transfer_insufficient_balance() {
        let self_wallet = get_self_wallet_id();

        let target = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_wallet('human', 'Overdraw Target')",
        )
        .unwrap()
        .unwrap();
        let target_id = target.0["id"].as_str().unwrap().to_string();

        // Try to transfer more than balance (self wallet starts at 0)
        Spi::run(&format!(
            "SELECT kerai.transfer_koi('{}'::uuid, '{}'::uuid, 999999, NULL)",
            self_wallet, target_id,
        ))
        .unwrap();
    }

    #[pg_test]
    fn test_wallet_history() {
        let self_wallet = mint_to_self(200);

        let target = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_wallet('agent', 'History Target')",
        )
        .unwrap()
        .unwrap();
        let target_id = target.0["id"].as_str().unwrap().to_string();

        Spi::run(&format!(
            "SELECT kerai.transfer_koi('{}'::uuid, '{}'::uuid, 50, 'history test')",
            self_wallet, target_id,
        ))
        .unwrap();

        let history = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.wallet_history('{}'::uuid, 10)",
            self_wallet,
        ))
        .unwrap()
        .unwrap();
        let arr = history.0.as_array().unwrap();
        assert!(arr.len() >= 2, "Should have at least 2 entries (mint + transfer), got {}", arr.len());
    }

    #[pg_test]
    fn test_get_wallet_balance() {
        let self_wallet = get_self_wallet_id();

        // Mint a known amount
        Spi::run(&format!(
            "SELECT kerai.mint_koi('{}'::uuid, 750, 'balance test', NULL, NULL)",
            self_wallet,
        ))
        .unwrap();

        let bal = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.get_wallet_balance('{}'::uuid)",
            self_wallet,
        ))
        .unwrap()
        .unwrap();
        assert!(bal.0["balance"].as_i64().unwrap() >= 750);
        assert!(bal.0["total_received"].as_i64().unwrap() >= 750);
    }

    #[pg_test]
    fn test_create_bounty() {
        // Need funds to create bounty
        let self_wallet = mint_to_self(5000);
        let _ = self_wallet;

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_bounty('pkg.auth', 'Fix login bug', 1000, 'cargo test', NULL)",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["description"].as_str().unwrap(), "Fix login bug");
        assert_eq!(obj["reward"].as_i64().unwrap(), 1000);
        assert_eq!(obj["status"].as_str().unwrap(), "open");
        assert!(obj.contains_key("id"));
    }

    #[pg_test]
    fn test_list_bounties() {
        mint_to_self(10000);

        Spi::run("SELECT kerai.create_bounty('pkg.a', 'Bounty A', 500, NULL, NULL)")
            .unwrap();
        Spi::run("SELECT kerai.create_bounty('pkg.b', 'Bounty B', 600, NULL, NULL)")
            .unwrap();

        // List all
        let all = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.list_bounties(NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let arr = all.0.as_array().unwrap();
        assert!(arr.len() >= 2, "Should have at least 2 bounties");

        // List with status filter
        let open = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.list_bounties('open', NULL)",
        )
        .unwrap()
        .unwrap();
        let oarr = open.0.as_array().unwrap();
        for b in oarr {
            assert_eq!(b["status"].as_str().unwrap(), "open");
        }
    }

    #[pg_test]
    fn test_claim_bounty() {
        mint_to_self(5000);

        let bounty = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_bounty('pkg.claim', 'Claim test', 500, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let bounty_id = bounty.0["id"].as_str().unwrap().to_string();

        // Create claimer wallet
        let claimer = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_wallet('human', 'Claimer')",
        )
        .unwrap()
        .unwrap();
        let claimer_id = claimer.0["id"].as_str().unwrap().to_string();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.claim_bounty('{}'::uuid, '{}'::uuid)",
            bounty_id, claimer_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(result.0["status"].as_str().unwrap(), "claimed");
    }

    #[pg_test]
    #[should_panic(expected = "cannot be claimed")]
    fn test_claim_bounty_already_claimed() {
        mint_to_self(5000);

        let bounty = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_bounty('pkg.double_claim', 'Double claim', 500, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let bounty_id = bounty.0["id"].as_str().unwrap().to_string();

        let claimer1 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_wallet('human', 'Claimer1')",
        )
        .unwrap()
        .unwrap();
        let claimer1_id = claimer1.0["id"].as_str().unwrap().to_string();

        let claimer2 = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_wallet('human', 'Claimer2')",
        )
        .unwrap()
        .unwrap();
        let claimer2_id = claimer2.0["id"].as_str().unwrap().to_string();

        // First claim succeeds
        Spi::run(&format!(
            "SELECT kerai.claim_bounty('{}'::uuid, '{}'::uuid)",
            bounty_id, claimer1_id,
        ))
        .unwrap();

        // Second claim should fail
        Spi::run(&format!(
            "SELECT kerai.claim_bounty('{}'::uuid, '{}'::uuid)",
            bounty_id, claimer2_id,
        ))
        .unwrap();
    }

    #[pg_test]
    fn test_settle_bounty() {
        mint_to_self(5000);

        let bounty = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_bounty('pkg.settle', 'Settle test', 1000, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let bounty_id = bounty.0["id"].as_str().unwrap().to_string();

        let claimer = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_wallet('human', 'Settler')",
        )
        .unwrap()
        .unwrap();
        let claimer_id = claimer.0["id"].as_str().unwrap().to_string();

        // Claim
        Spi::run(&format!(
            "SELECT kerai.claim_bounty('{}'::uuid, '{}'::uuid)",
            bounty_id, claimer_id,
        ))
        .unwrap();

        // Settle
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.settle_bounty('{}'::uuid)",
            bounty_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(result.0["status"].as_str().unwrap(), "paid");
        assert_eq!(result.0["reward"].as_i64().unwrap(), 1000);

        // Verify claimer received payment
        let bal = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.get_wallet_balance('{}'::uuid)",
            claimer_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(bal.0["balance"].as_i64().unwrap(), 1000);
    }

    #[pg_test]
    #[should_panic(expected = "must be 'claimed' to settle")]
    fn test_settle_bounty_not_claimed() {
        mint_to_self(5000);

        let bounty = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_bounty('pkg.bad_settle', 'Bad settle', 500, NULL, NULL)",
        )
        .unwrap()
        .unwrap();
        let bounty_id = bounty.0["id"].as_str().unwrap().to_string();

        // Try to settle without claiming first
        Spi::run(&format!(
            "SELECT kerai.settle_bounty('{}'::uuid)",
            bounty_id,
        ))
        .unwrap();
    }

    // --- Plan 13: Native Currency tests ---

    /// Helper: generate a test Ed25519 keypair. Returns (signing_key, public_key_hex).
    fn generate_currency_keypair() -> (ed25519_dalek::SigningKey, String) {
        let mut rng = rand::rngs::OsRng;
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();
        let pk_hex: String = verifying_key
            .as_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        (signing_key, pk_hex)
    }

    #[pg_test]
    fn test_register_wallet_currency() {
        let (_sk, pk_hex) = generate_currency_keypair();
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.register_wallet('{}', 'human', 'Alice Currency')",
            pk_hex,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["wallet_type"].as_str().unwrap(), "human");
        assert_eq!(obj["label"].as_str().unwrap(), "Alice Currency");
        assert!(obj.contains_key("id"));
        assert!(obj.contains_key("key_fingerprint"));
        assert_eq!(obj["nonce"].as_i64().unwrap(), 0);
    }

    #[pg_test]
    #[should_panic(expected = "Invalid public key")]
    fn test_register_wallet_invalid_key() {
        Spi::run("SELECT kerai.register_wallet('deadbeef', 'human', NULL)")
            .unwrap();
    }

    #[pg_test]
    #[should_panic(expected = "duplicate key value violates unique constraint")]
    fn test_register_wallet_duplicate_key() {
        let (_sk, pk_hex) = generate_currency_keypair();
        Spi::run(&format!(
            "SELECT kerai.register_wallet('{}', 'human', 'First')",
            pk_hex,
        ))
        .unwrap();
        // Same pubkey again should fail (unique fingerprint)
        Spi::run(&format!(
            "SELECT kerai.register_wallet('{}', 'external', 'Second')",
            pk_hex,
        ))
        .unwrap();
    }

    #[pg_test]
    fn test_signed_transfer() {
        use ed25519_dalek::Signer;

        let (sk, pk_hex) = generate_currency_keypair();

        // Register wallet with this keypair
        let wallet = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.register_wallet('{}', 'human', 'Signer')",
            pk_hex,
        ))
        .unwrap()
        .unwrap();
        let from_id = wallet.0["id"].as_str().unwrap().to_string();

        // Mint some Koi to the registered wallet
        Spi::run(&format!(
            "SELECT kerai.mint_koi('{}'::uuid, 500, 'seed', NULL, NULL)",
            from_id,
        ))
        .unwrap();

        // Get self wallet as destination
        let to_id = get_self_wallet_id();

        // Sign the transfer message: "transfer:{from}:{to}:{amount}:{nonce}"
        let message = format!("transfer:{}:{}:100:1", from_id, to_id);
        let signature = sk.sign(message.as_bytes());
        let sig_hex: String = signature
            .to_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.signed_transfer('{}'::uuid, '{}'::uuid, 100, 1, '{}', 'test payment')",
            from_id, to_id, sig_hex,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(result.0["amount"].as_i64().unwrap(), 100);

        // Verify sender balance decreased
        let sender_bal = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.get_wallet_balance('{}'::uuid)",
            from_id,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(sender_bal.0["balance"].as_i64().unwrap(), 400);
    }

    #[pg_test]
    #[should_panic(expected = "Invalid signature")]
    fn test_signed_transfer_bad_signature() {
        let (_sk, pk_hex) = generate_currency_keypair();
        let wallet = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.register_wallet('{}', 'human', 'BadSig')",
            pk_hex,
        ))
        .unwrap()
        .unwrap();
        let from_id = wallet.0["id"].as_str().unwrap().to_string();

        Spi::run(&format!(
            "SELECT kerai.mint_koi('{}'::uuid, 100, 'seed', NULL, NULL)",
            from_id,
        ))
        .unwrap();

        let to_id = get_self_wallet_id();
        // Bad signature (all zeros)
        let bad_sig = "00".repeat(64);

        Spi::run(&format!(
            "SELECT kerai.signed_transfer('{}'::uuid, '{}'::uuid, 50, 1, '{}', NULL)",
            from_id, to_id, bad_sig,
        ))
        .unwrap();
    }

    #[pg_test]
    #[should_panic(expected = "Invalid nonce")]
    fn test_signed_transfer_bad_nonce() {
        use ed25519_dalek::Signer;

        let (sk, pk_hex) = generate_currency_keypair();
        let wallet = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.register_wallet('{}', 'human', 'BadNonce')",
            pk_hex,
        ))
        .unwrap()
        .unwrap();
        let from_id = wallet.0["id"].as_str().unwrap().to_string();

        Spi::run(&format!(
            "SELECT kerai.mint_koi('{}'::uuid, 100, 'seed', NULL, NULL)",
            from_id,
        ))
        .unwrap();

        let to_id = get_self_wallet_id();
        // Wrong nonce (5 instead of 1)
        let message = format!("transfer:{}:{}:50:5", from_id, to_id);
        let signature = sk.sign(message.as_bytes());
        let sig_hex: String = signature
            .to_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();

        Spi::run(&format!(
            "SELECT kerai.signed_transfer('{}'::uuid, '{}'::uuid, 50, 5, '{}', NULL)",
            from_id, to_id, sig_hex,
        ))
        .unwrap();
    }

    #[pg_test]
    #[should_panic(expected = "Insufficient balance")]
    fn test_signed_transfer_insufficient_balance() {
        use ed25519_dalek::Signer;

        let (sk, pk_hex) = generate_currency_keypair();
        let wallet = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.register_wallet('{}', 'human', 'Broke')",
            pk_hex,
        ))
        .unwrap()
        .unwrap();
        let from_id = wallet.0["id"].as_str().unwrap().to_string();

        // No mint — wallet has 0 balance
        let to_id = get_self_wallet_id();
        let message = format!("transfer:{}:{}:100:1", from_id, to_id);
        let signature = sk.sign(message.as_bytes());
        let sig_hex: String = signature
            .to_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();

        Spi::run(&format!(
            "SELECT kerai.signed_transfer('{}'::uuid, '{}'::uuid, 100, 1, '{}', NULL)",
            from_id, to_id, sig_hex,
        ))
        .unwrap();
    }

    #[pg_test]
    fn test_total_supply() {
        let wallet_id = get_self_wallet_id();
        Spi::run(&format!(
            "SELECT kerai.mint_koi('{}'::uuid, 1000, 'supply test', NULL, NULL)",
            wallet_id,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>("SELECT kerai.total_supply()")
            .unwrap()
            .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["total_supply"].as_i64().unwrap() >= 1000);
        assert!(obj["total_minted"].as_i64().unwrap() >= 1000);
        assert!(obj["total_transactions"].as_i64().unwrap() >= 1);
    }

    #[pg_test]
    fn test_wallet_share() {
        let wallet_id = get_self_wallet_id();
        Spi::run(&format!(
            "SELECT kerai.mint_koi('{}'::uuid, 500, 'share test', NULL, NULL)",
            wallet_id,
        ))
        .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.wallet_share('{}'::uuid)",
            wallet_id,
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["balance"].as_i64().unwrap() > 0);
        assert!(obj["total_supply"].as_i64().unwrap() > 0);
        let share = obj["share"].as_str().unwrap();
        let share_val: f64 = share.parse().unwrap();
        assert!(share_val > 0.0 && share_val <= 1.0, "Share should be between 0 and 1, got {}", share_val);
    }

    #[pg_test]
    fn test_supply_info() {
        let result = Spi::get_one::<pgrx::JsonB>("SELECT kerai.supply_info()")
            .unwrap()
            .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj.contains_key("total_supply"));
        assert!(obj.contains_key("wallet_count"));
        assert!(obj.contains_key("top_holders"));
        assert!(obj.contains_key("recent_mints"));
        assert!(obj["wallet_count"].as_i64().unwrap() >= 1);
    }

    #[pg_test]
    fn test_mint_reward() {
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.mint_reward('parse_file', '{\"file\": \"test.rs\"}'::jsonb)",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["work_type"].as_str().unwrap(), "parse_file");
        assert_eq!(obj["reward"].as_i64().unwrap(), 10_000_000_000); // 10 Koi in nKoi
        assert!(obj.contains_key("ledger_id"));
        assert!(obj.contains_key("wallet_id"));

        // Verify reward_log entry exists
        let log_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.reward_log WHERE work_type = 'parse_file'",
        )
        .unwrap()
        .unwrap();
        assert!(log_count >= 1, "Should have at least 1 reward_log entry");
    }

    #[pg_test]
    fn test_mint_reward_disabled() {
        // Disable a work type
        Spi::run("UPDATE kerai.reward_schedule SET enabled = false WHERE work_type = 'peer_sync'")
            .unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.mint_reward('peer_sync', NULL)",
        )
        .unwrap()
        .unwrap();
        assert!(result.0.is_null(), "Disabled work type should return null");
    }

    #[pg_test]
    fn test_evaluate_mining() {
        let result = Spi::get_one::<pgrx::JsonB>("SELECT kerai.evaluate_mining()")
            .unwrap()
            .unwrap();
        let obj = result.0.as_object().unwrap();
        assert!(obj["evaluated"].as_bool().unwrap());
        assert!(obj.contains_key("mints"));
    }

    #[pg_test]
    fn test_get_reward_schedule() {
        let result = Spi::get_one::<pgrx::JsonB>("SELECT kerai.get_reward_schedule()")
            .unwrap()
            .unwrap();
        let arr = result.0.as_array().unwrap();
        assert!(arr.len() >= 6, "Should have at least 6 seed schedule entries, got {}", arr.len());

        // Verify parse_file entry
        let parse_file = arr.iter().find(|v| v["work_type"].as_str() == Some("parse_file")).unwrap();
        assert_eq!(parse_file["reward"].as_i64().unwrap(), 10_000_000_000); // 10 Koi in nKoi
        assert!(parse_file["enabled"].as_bool().unwrap());
    }

    #[pg_test]
    fn test_set_reward() {
        // Create a new reward type
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.set_reward('custom_work', 42, true)",
        )
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["work_type"].as_str().unwrap(), "custom_work");
        assert_eq!(obj["reward"].as_i64().unwrap(), 42);
        assert!(obj["enabled"].as_bool().unwrap());

        // Update it
        let updated = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.set_reward('custom_work', 100, false)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.0["reward"].as_i64().unwrap(), 100);
        assert!(!updated.0["enabled"].as_bool().unwrap());
    }

    #[pg_test]
    fn test_auto_mint_on_parse() {
        // Get supply before
        let before = Spi::get_one::<pgrx::JsonB>("SELECT kerai.total_supply()")
            .unwrap()
            .unwrap();
        let supply_before = before.0["total_supply"].as_i64().unwrap();

        // Parse source (should trigger auto-mint)
        Spi::run("SELECT kerai.parse_source('fn auto_mint_test() {}', 'auto_mint.rs')")
            .unwrap();

        // Get supply after
        let after = Spi::get_one::<pgrx::JsonB>("SELECT kerai.total_supply()")
            .unwrap()
            .unwrap();
        let supply_after = after.0["total_supply"].as_i64().unwrap();

        assert!(
            supply_after > supply_before,
            "Supply should increase after parsing: before={}, after={}",
            supply_before,
            supply_after,
        );
    }

    #[pg_test]
    fn test_status_includes_supply() {
        let status = Spi::get_one::<pgrx::JsonB>("SELECT kerai.status()")
            .unwrap()
            .unwrap();
        let obj = status.0.as_object().unwrap();
        assert!(obj.contains_key("total_supply"), "Status should include total_supply");
        assert!(obj.contains_key("instance_balance"), "Status should include instance_balance");
    }

    // ────── MicroGPT tests ──────

    #[pg_test]
    fn test_tensor_matmul() {
        use crate::microgpt::tensor::Tensor;
        let a = Tensor {
            data: vec![1.0, 2.0, 3.0, 4.0],
            shape: vec![2, 2],
        };
        let b = Tensor {
            data: vec![5.0, 6.0, 7.0, 8.0],
            shape: vec![2, 2],
        };
        let c = a.matmul(&b);
        assert_eq!(c.data, vec![19.0, 22.0, 43.0, 50.0]);
        assert_eq!(c.shape, vec![2, 2]);
    }

    #[pg_test]
    fn test_tensor_softmax() {
        use crate::microgpt::tensor::Tensor;
        let t = Tensor {
            data: vec![1.0, 2.0, 3.0, 100.0, 200.0, 300.0],
            shape: vec![2, 3],
        };
        let s = t.softmax();
        // Each row should sum to 1.0
        let sum1: f32 = s.data[0..3].iter().sum();
        let sum2: f32 = s.data[3..6].iter().sum();
        assert!((sum1 - 1.0).abs() < 1e-5, "Row 1 sum: {}", sum1);
        assert!((sum2 - 1.0).abs() < 1e-5, "Row 2 sum: {}", sum2);
    }

    #[pg_test]
    fn test_forward_pass_shape() {
        use crate::microgpt::model::{MicroGPT, ModelConfig};
        let config = ModelConfig {
            vocab_size: 20,
            dim: 16,
            n_heads: 4,
            n_layers: 1,
            context_len: 8,
        };
        let model = MicroGPT::new(config);
        let tokens = vec![0, 5, 10, 15];
        let (logits, _cache) = model.forward(&tokens);
        assert_eq!(logits.shape, vec![4, 20], "Logits shape: {:?}", logits.shape);
    }

    #[pg_test]
    fn test_weight_roundtrip() {
        use crate::microgpt::model::{MicroGPT, ModelConfig};
        let config = ModelConfig {
            vocab_size: 10,
            dim: 8,
            n_heads: 2,
            n_layers: 1,
            context_len: 4,
        };
        let model = MicroGPT::new(config.clone());
        let weight_map = model.to_weight_map();
        let model2 = MicroGPT::from_weight_map(config, &weight_map);
        let tokens = vec![0, 1, 2];
        let (logits1, _) = model.forward(&tokens);
        let (logits2, _) = model2.forward(&tokens);
        assert_eq!(logits1.data, logits2.data, "Roundtrip should produce identical logits");
    }

    #[pg_test]
    fn test_train_loss_decreases() {
        use crate::microgpt::model::{MicroGPT, ModelConfig};
        use crate::microgpt::optimizer::Adam;
        let config = ModelConfig {
            vocab_size: 10,
            dim: 16,
            n_heads: 4,
            n_layers: 1,
            context_len: 8,
        };
        let mut model = MicroGPT::new(config);
        let mut optimizer = Adam::new(model.param_count(), 0.01);
        // Simple repeating sequence: 0,1,2,...,9,0,1,2,...
        let sequences: Vec<Vec<usize>> = (0..10)
            .map(|start| (start..start + 6).map(|i| i % 10).collect())
            .collect();
        let mut first_loss = 0.0f32;
        let mut last_loss = 0.0f32;
        for step in 0..50 {
            let loss = model.train_step(&sequences, &mut optimizer);
            if step == 0 {
                first_loss = loss;
            }
            last_loss = loss;
        }
        assert!(
            last_loss < first_loss,
            "Loss should decrease: first={:.4} last={:.4}",
            first_loss,
            last_loss
        );
    }

    #[pg_test]
    fn test_predict_next_returns_results() {
        use crate::microgpt::model::{MicroGPT, ModelConfig};
        let config = ModelConfig {
            vocab_size: 10,
            dim: 8,
            n_heads: 2,
            n_layers: 1,
            context_len: 4,
        };
        let model = MicroGPT::new(config);
        let preds = model.predict_next(&[0, 1, 2], 5);
        assert!(!preds.is_empty(), "Should return predictions");
        assert!(preds.len() <= 5, "Should return at most 5");
        // Probabilities should sum roughly to 1 (top-k subset)
        let sum: f32 = preds.iter().map(|(_, p)| p).sum();
        assert!(sum <= 1.0 + 1e-5, "Probabilities sum: {}", sum);
    }

    #[pg_test]
    fn test_create_model() {
        // Parse some source to populate nodes
        Spi::run(
            "SELECT kerai.parse_source('fn hello() { } fn world() { }', 'test_model.rs')",
        )
        .unwrap();

        // Create an agent
        Spi::run(
            "INSERT INTO kerai.agents (name, kind, wallet_id)
             VALUES ('model_test_agent', 'llm',
                     (SELECT id FROM kerai.wallets WHERE instance_id = (SELECT id FROM kerai.instances WHERE is_self = true) LIMIT 1))
             ON CONFLICT (name) DO NOTHING",
        )
        .unwrap();

        // Create model
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.create_model('model_test_agent')",
        )
        .unwrap()
        .unwrap();

        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["status"].as_str().unwrap(), "created");
        assert!(obj["vocab_size"].as_u64().unwrap() > 0);
        assert!(obj["param_count"].as_u64().unwrap() > 0);

        // Verify weights stored in DB
        let weight_count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.model_weights
             WHERE agent_id = (SELECT id FROM kerai.agents WHERE name = 'model_test_agent')",
        )
        .unwrap()
        .unwrap();
        assert!(weight_count > 0, "Weights should be stored in DB");

        // Verify vocab stored in DB
        let vocab_count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.model_vocab
             WHERE model_id = (SELECT id FROM kerai.agents WHERE name = 'model_test_agent')",
        )
        .unwrap()
        .unwrap();
        assert!(vocab_count > 0, "Vocab should be stored in DB");
    }

    #[pg_test]
    fn test_model_info() {
        Spi::run(
            "SELECT kerai.parse_source('struct Foo { x: i32 }', 'test_info.rs')",
        )
        .unwrap();
        Spi::run(
            "INSERT INTO kerai.agents (name, kind, wallet_id)
             VALUES ('info_agent', 'llm',
                     (SELECT id FROM kerai.wallets WHERE instance_id = (SELECT id FROM kerai.instances WHERE is_self = true) LIMIT 1))
             ON CONFLICT (name) DO NOTHING",
        )
        .unwrap();
        Spi::run("SELECT kerai.create_model('info_agent')").unwrap();

        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.model_info('info_agent')",
        )
        .unwrap()
        .unwrap();

        let obj = result.0.as_object().unwrap();
        assert_eq!(obj["agent"].as_str().unwrap(), "info_agent");
        assert!(obj["vocab_size"].as_u64().unwrap() > 0);
        assert!(obj.contains_key("dim"));
        assert!(obj.contains_key("training_runs"));
    }

    #[pg_test]
    fn test_delete_model() {
        Spi::run(
            "SELECT kerai.parse_source('fn zz() {}', 'test_delete.rs')",
        )
        .unwrap();
        Spi::run(
            "INSERT INTO kerai.agents (name, kind, wallet_id)
             VALUES ('del_agent', 'llm',
                     (SELECT id FROM kerai.wallets WHERE instance_id = (SELECT id FROM kerai.instances WHERE is_self = true) LIMIT 1))
             ON CONFLICT (name) DO NOTHING",
        )
        .unwrap();
        Spi::run("SELECT kerai.create_model('del_agent')").unwrap();

        // Delete
        let result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.delete_model('del_agent')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.0["status"].as_str().unwrap(), "deleted");

        // Verify weights removed
        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.model_weights
             WHERE agent_id = (SELECT id FROM kerai.agents WHERE name = 'del_agent')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(count, 0);
    }

    #[pg_test]
    fn test_tensor_byte_roundtrip() {
        use crate::microgpt::tensor::Tensor;
        let t = Tensor {
            data: vec![3.14, -2.71, 0.0, 1e10, -1e-10, f32::MAX],
            shape: vec![2, 3],
        };
        let bytes = t.to_bytes();
        let t2 = Tensor::from_bytes(&bytes, vec![2, 3]);
        for (a, b) in t.data.iter().zip(t2.data.iter()) {
            assert_eq!(a.to_bits(), b.to_bits(), "Byte roundtrip should be exact");
        }
    }

    // --- Comment handling tests ---

    #[pg_test]
    fn test_comment_grouping() {
        // 3 consecutive // lines should become 1 comment_block node
        let source = "// line one\n// line two\n// line three\nfn foo() {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_grouping.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let block_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'comment_block'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(block_count, 1, "3 consecutive // lines should be 1 comment_block");

        // Verify it has 3 lines in content (newline-separated)
        let content = Spi::get_one::<String>(
            "SELECT content FROM kerai.nodes WHERE kind = 'comment_block' LIMIT 1",
        )
        .unwrap()
        .unwrap();
        let line_count = content.split('\n').count();
        assert_eq!(line_count, 3, "comment_block should have 3 lines");
    }

    #[pg_test]
    fn test_comment_placement_above() {
        let source = "// helper\nfn foo() {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_above.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let placement = Spi::get_one::<String>(
            "SELECT metadata->>'placement' FROM kerai.nodes WHERE kind = 'comment' \
             AND content = 'helper' LIMIT 1",
        )
        .unwrap()
        .unwrap();
        assert_eq!(placement, "above", "Comment directly above fn should be placement=above");

        // Should have a documents edge
        let edge_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.edges e \
             JOIN kerai.nodes n ON e.source_id = n.id \
             WHERE n.kind = 'comment' AND n.content = 'helper' \
             AND e.relation = 'documents'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(edge_count, 1, "Above comment should have documents edge");
    }

    #[pg_test]
    fn test_comment_placement_eof() {
        let source = "fn foo() {}\n// trailing at end\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_eof.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let placement = Spi::get_one::<String>(
            "SELECT metadata->>'placement' FROM kerai.nodes WHERE kind = 'comment' \
             AND content = 'trailing at end' LIMIT 1",
        )
        .unwrap()
        .unwrap();
        assert_eq!(placement, "eof", "Comment at end with no following AST node should be eof");

        // Eof comments should have NO documents edge
        let edge_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.edges e \
             JOIN kerai.nodes n ON e.source_id = n.id \
             WHERE n.kind = 'comment' AND n.content = 'trailing at end' \
             AND e.relation = 'documents'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(edge_count, 0, "Eof comment should have no documents edge");
    }

    #[pg_test]
    fn test_comment_not_in_string() {
        // The // is inside a string literal on a single line — should not be extracted
        let source = "fn foo() { let s = \"// not a comment\"; }\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_string.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let comment_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes \
             WHERE kind IN ('comment', 'comment_block') \
             AND content LIKE '%not a comment%'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(comment_count, 0, "// inside string literal should not be extracted");
    }

    #[pg_test]
    fn test_normalization_crlf() {
        // CRLF source should parse correctly after normalization
        let source = "fn hello() {\r\n    let x = 1;\r\n}\r\n";
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.parse_source('{}', 'test_crlf.rs')",
            sql_escape(source),
        ))
        .unwrap()
        .unwrap();
        let obj = result.0.as_object().unwrap();
        let node_count = obj["nodes"].as_u64().unwrap();
        assert!(node_count > 0, "CRLF source should parse successfully");

        let fn_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'fn' AND content = 'hello'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(fn_count, 1, "Should find fn hello after CRLF normalization");
    }

    #[pg_test]
    fn test_normalization_blank_lines() {
        // Multiple blank lines between fns should be collapsed
        let source = "fn a() {}\n\n\n\n\nfn b() {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_blanks.rs')",
            sql_escape(source),
        ))
        .unwrap();

        // Both fns should be parsed
        let fn_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes WHERE kind = 'fn'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(fn_count, 2, "Both fns should be parsed after blank line collapse");
    }

    #[pg_test]
    fn test_roundtrip_with_comments() {
        let source = "// above comment\nfn foo() {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_rt_comments.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let file_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes WHERE kind = 'file' AND content = 'test_rt_comments.rs'",
        )
        .unwrap()
        .unwrap();

        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_file('{}'::uuid)",
            sql_escape(&file_id),
        ))
        .unwrap()
        .unwrap();

        assert!(
            reconstructed.contains("// above comment"),
            "Reconstructed source should contain the above comment, got: {}",
            reconstructed,
        );
        assert!(
            reconstructed.contains("fn foo()"),
            "Reconstructed source should contain fn foo()",
        );
    }

    // --- Plan 16: Reconstruction Intelligence tests ---

    #[pg_test]
    fn test_import_sorting_in_reconstruction() {
        // Source with imports in wrong order
        let source = "use crate::foo;\nuse std::io;\nuse serde::Deserialize;\nfn bar() {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_import_sort.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let file_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes WHERE kind = 'file' AND content = 'test_import_sort.rs'",
        )
        .unwrap()
        .unwrap();

        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_file('{}'::uuid)",
            sql_escape(&file_id),
        ))
        .unwrap()
        .unwrap();

        // std should come before serde, serde before crate::
        let std_pos = reconstructed.find("std::io").expect("should contain std::io");
        let serde_pos = reconstructed.find("serde").expect("should contain serde");
        let crate_pos = reconstructed.find("crate::foo").expect("should contain crate::foo");
        assert!(
            std_pos < serde_pos && serde_pos < crate_pos,
            "Imports should be sorted: std < external < crate, got:\n{}",
            reconstructed,
        );
    }

    #[pg_test]
    fn test_derive_ordering_in_reconstruction() {
        let source = "#[derive(Serialize, Clone, Debug)]\nstruct Foo { x: i32 }\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_derive_order.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let file_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes WHERE kind = 'file' AND content = 'test_derive_order.rs'",
        )
        .unwrap()
        .unwrap();

        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_file('{}'::uuid)",
            sql_escape(&file_id),
        ))
        .unwrap()
        .unwrap();

        // Derives should be alphabetically sorted
        assert!(
            reconstructed.contains("Clone, Debug, Serialize")
                || reconstructed.contains("Clone , Debug , Serialize"),
            "Derives should be alphabetically sorted, got:\n{}",
            reconstructed,
        );
    }

    #[pg_test]
    fn test_suggestion_created_for_string_param() {
        let source = "fn process(s: &String) {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_suggest_str.rs')",
            sql_escape(source),
        ))
        .unwrap();

        // Check that a suggestion node was created
        let suggestion_count = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM kerai.nodes \
             WHERE kind = 'suggestion' AND metadata->>'rule' = 'prefer_str_slice'",
        )
        .unwrap()
        .unwrap();

        assert!(
            suggestion_count > 0,
            "Should create a prefer_str_slice suggestion for &String param",
        );
    }

    #[pg_test]
    fn test_suggestion_emitted_in_reconstruction() {
        let source = "fn process(s: &String) {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_suggest_emit.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let file_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes WHERE kind = 'file' AND content = 'test_suggest_emit.rs'",
        )
        .unwrap()
        .unwrap();

        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_file_with_options('{}'::uuid, '{{\"suggestions\": true}}'::jsonb)",
            sql_escape(&file_id),
        ))
        .unwrap()
        .unwrap();

        assert!(
            reconstructed.contains("// kerai:") && reconstructed.contains("prefer_str_slice"),
            "Reconstructed source should contain kerai suggestion comment, got:\n{}",
            reconstructed,
        );
    }

    #[pg_test]
    fn test_suggestion_not_emitted_with_skip_flag() {
        let source = "fn process(s: &String) {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_suggest_skip.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let file_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes WHERE kind = 'file' AND content = 'test_suggest_skip.rs'",
        )
        .unwrap()
        .unwrap();

        // Reconstruct with suggestions disabled
        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_file_with_options('{}'::uuid, '{{\"suggestions\": false}}'::jsonb)",
            sql_escape(&file_id),
        ))
        .unwrap()
        .unwrap();

        assert!(
            !reconstructed.contains("// kerai:"),
            "Reconstructed source should NOT contain kerai suggestion when disabled, got:\n{}",
            reconstructed,
        );
    }

    #[pg_test]
    fn test_reconstruct_with_options_no_sorting() {
        let source = "use crate::foo;\nuse std::io;\nfn bar() {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_no_sort.rs')",
            sql_escape(source),
        ))
        .unwrap();

        let file_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes WHERE kind = 'file' AND content = 'test_no_sort.rs'",
        )
        .unwrap()
        .unwrap();

        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_file_with_options('{}'::uuid, '{{\"sort_imports\": false, \"suggestions\": false}}'::jsonb)",
            sql_escape(&file_id),
        ))
        .unwrap()
        .unwrap();

        // Without sorting, crate:: should appear before std:: (original order)
        let crate_pos = reconstructed.find("crate::foo");
        let std_pos = reconstructed.find("std::io");
        if let (Some(c), Some(s)) = (crate_pos, std_pos) {
            assert!(
                c < s,
                "Without sorting, imports should stay in original order, got:\n{}",
                reconstructed,
            );
        }
    }

    #[pg_test]
    fn test_kerai_skip_flag_parsed() {
        let source = "// kerai:skip-sort-imports\nuse crate::foo;\nuse std::io;\nfn bar() {}\n";
        Spi::run(&format!(
            "SELECT kerai.parse_source('{}', 'test_skip_flag.rs')",
            sql_escape(source),
        ))
        .unwrap();

        // Check that the flag is stored in the file node metadata
        let has_flag = Spi::get_one::<bool>(
            "SELECT (metadata->'kerai_flags'->>'skip-sort-imports')::boolean \
             FROM kerai.nodes WHERE kind = 'file' AND content = 'test_skip_flag.rs'",
        )
        .unwrap()
        .unwrap_or(false);

        assert!(has_flag, "File node should have kerai_flags.skip-sort-imports = true");
    }

    // ── Go parser tests ──────────────────────────────────────────────────

    #[pg_test]
    fn test_parse_go_source_basic() {
        let source = r#"package main

import "fmt"

func main() {
    fmt.Println("hello")
}
"#;
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.parse_go_source('{}', 'hello.go')",
            sql_escape(source),
        ))
        .unwrap()
        .unwrap();

        let nodes = result.0.get("nodes").and_then(|v| v.as_u64()).unwrap_or(0);
        assert!(nodes > 0, "parse_go_source should produce nodes, got {}", nodes);
    }

    #[pg_test]
    fn test_go_func_node_kind() {
        let source = r#"package main

func Hello() {}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_go_source('{}', 'func_kind.go')",
            sql_escape(source),
        ))
        .unwrap();

        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'go_func' AND content = 'Hello'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(count, 1, "Should have one go_func node named Hello");
    }

    #[pg_test]
    fn test_go_exported_metadata() {
        let source = r#"package main

func Exported() {}
func unexported() {}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_go_source('{}', 'export_test.go')",
            sql_escape(source),
        ))
        .unwrap();

        let exported = Spi::get_one::<bool>(
            "SELECT (metadata->>'exported')::boolean FROM kerai.nodes \
             WHERE kind = 'go_func' AND content = 'Exported'",
        )
        .unwrap()
        .unwrap_or(false);
        assert!(exported, "Exported function should have exported=true");

        let unexported = Spi::get_one::<bool>(
            "SELECT (metadata->>'exported')::boolean FROM kerai.nodes \
             WHERE kind = 'go_func' AND content = 'unexported'",
        )
        .unwrap()
        .unwrap_or(true);
        assert!(!unexported, "unexported function should have exported=false");
    }

    #[pg_test]
    fn test_go_struct_fields() {
        let source = r#"package main

type User struct {
    Name  string
    Email string
    Age   int
}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_go_source('{}', 'struct_test.go')",
            sql_escape(source),
        ))
        .unwrap();

        let field_count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'go_field' \
             AND language = 'go'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(field_count, 3, "Struct should have 3 fields, got {}", field_count);
    }

    #[pg_test]
    fn test_go_import_specs() {
        let source = r#"package main

import (
    "fmt"
    "os"
    "strings"
)

func main() {}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_go_source('{}', 'import_test.go')",
            sql_escape(source),
        ))
        .unwrap();

        let import_count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'go_import_spec'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(import_count, 3, "Should have 3 import specs, got {}", import_count);
    }

    #[pg_test]
    fn test_go_method_receiver() {
        let source = r#"package main

type Server struct{}

func (s *Server) Start() {}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_go_source('{}', 'method_test.go')",
            sql_escape(source),
        ))
        .unwrap();

        let has_receiver = Spi::get_one::<bool>(
            "SELECT (metadata->>'pointer_receiver')::boolean FROM kerai.nodes \
             WHERE kind = 'go_method' AND content = 'Start'",
        )
        .unwrap()
        .unwrap_or(false);

        assert!(has_receiver, "Method should have pointer_receiver=true");
    }

    #[pg_test]
    fn test_go_comment_documents_edge() {
        let source = r#"package main

// Hello prints a greeting.
func Hello() {}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_go_source('{}', 'comment_edge.go')",
            sql_escape(source),
        ))
        .unwrap();

        let doc_edge = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.edges e \
             JOIN kerai.nodes t ON e.target_id = t.id \
             WHERE e.relation = 'documents' \
             AND t.kind = 'go_func' AND t.content = 'Hello'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(doc_edge, 1, "Comment above Hello should create 'documents' edge");
    }

    #[pg_test]
    fn test_go_reconstruct_roundtrip() {
        let source = r#"package main

import "fmt"

// Hello prints a greeting.
func Hello(name string) {
    fmt.Println("Hello, " + name)
}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_go_source('{}', 'roundtrip.go')",
            sql_escape(source),
        ))
        .unwrap();

        let file_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes \
             WHERE kind = 'file' AND content = 'roundtrip.go' AND language = 'go'",
        )
        .unwrap()
        .unwrap();

        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_go_file('{}'::uuid)",
            sql_escape(&file_id),
        ))
        .unwrap()
        .unwrap();

        assert!(
            reconstructed.contains("package main"),
            "Reconstructed should contain package declaration"
        );
        assert!(
            reconstructed.contains("func Hello"),
            "Reconstructed should contain Hello function"
        );
    }

    #[pg_test]
    fn test_go_suggestion_exported_no_doc() {
        let source = r#"package main

func ExportedNoDoc() {}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_go_source('{}', 'suggest_test.go')",
            sql_escape(source),
        ))
        .unwrap();

        let suggestion = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes \
             WHERE kind = 'suggestion' AND language = 'go' \
             AND metadata->>'rule' = 'go_exported_no_doc'",
        )
        .unwrap()
        .unwrap_or(0);

        assert!(suggestion > 0, "Exported function without doc should trigger suggestion");
    }

    // ── C parser tests ───────────────────────────────────────────────────

    #[pg_test]
    fn test_parse_c_source_basic() {
        let source = r#"#include <stdio.h>

int main(void) {
    printf("hello\n");
    return 0;
}
"#;
        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.parse_c_source('{}', 'hello.c')",
            sql_escape(source),
        ))
        .unwrap()
        .unwrap();

        let nodes = result.0.get("nodes").and_then(|v| v.as_u64()).unwrap_or(0);
        assert!(nodes > 0, "parse_c_source should produce nodes, got {}", nodes);
    }

    #[pg_test]
    fn test_c_function_node_kind() {
        let source = r#"int main(void) {
    return 0;
}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'func_kind.c')",
            sql_escape(source),
        ))
        .unwrap();

        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'c_function' AND content = 'main'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(count, 1, "Should have one c_function node named main");
    }

    #[pg_test]
    fn test_c_static_metadata() {
        let source = r#"static int helper(int x) {
    return x * 2;
}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'static_test.c')",
            sql_escape(source),
        ))
        .unwrap();

        let is_static = Spi::get_one::<bool>(
            "SELECT (metadata->>'static')::boolean FROM kerai.nodes \
             WHERE kind = 'c_function' AND content = 'helper'",
        )
        .unwrap()
        .unwrap_or(false);

        assert!(is_static, "static function should have static=true metadata");
    }

    #[pg_test]
    fn test_c_struct_fields() {
        let source = r#"struct Point {
    int x;
    int y;
    int z;
};
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'struct_test.c')",
            sql_escape(source),
        ))
        .unwrap();

        let field_count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'c_field' AND language = 'c'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(field_count, 3, "Struct should have 3 fields, got {}", field_count);
    }

    #[pg_test]
    fn test_c_enum_enumerators() {
        let source = r#"enum Color { RED, GREEN, BLUE };
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'enum_test.c')",
            sql_escape(source),
        ))
        .unwrap();

        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'c_enumerator' AND language = 'c'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(count, 3, "Enum should have 3 enumerators, got {}", count);
    }

    #[pg_test]
    fn test_c_include_metadata() {
        let source = r#"#include <stdio.h>
#include "myheader.h"
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'include_test.c')",
            sql_escape(source),
        ))
        .unwrap();

        let system = Spi::get_one::<bool>(
            "SELECT (metadata->>'system')::boolean FROM kerai.nodes \
             WHERE kind = 'c_include' AND metadata->>'path' LIKE '%stdio.h%'",
        )
        .unwrap()
        .unwrap_or(false);

        assert!(system, "#include <stdio.h> should have system=true");

        let user_include = Spi::get_one::<bool>(
            "SELECT (metadata->>'system')::boolean FROM kerai.nodes \
             WHERE kind = 'c_include' AND metadata->>'path' LIKE '%myheader.h%'",
        )
        .unwrap()
        .unwrap_or(true);

        assert!(!user_include, "#include \"myheader.h\" should have system=false");
    }

    #[pg_test]
    fn test_c_define_metadata() {
        let source = r#"#define MAX_SIZE 100
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'define_test.c')",
            sql_escape(source),
        ))
        .unwrap();

        let name = Spi::get_one::<String>(
            "SELECT metadata->>'name' FROM kerai.nodes \
             WHERE kind = 'c_define' AND language = 'c'",
        )
        .unwrap()
        .unwrap_or_default();

        assert_eq!(name, "MAX_SIZE", "Define should have name=MAX_SIZE");

        let value = Spi::get_one::<String>(
            "SELECT metadata->>'value' FROM kerai.nodes \
             WHERE kind = 'c_define' AND language = 'c'",
        )
        .unwrap()
        .unwrap_or_default();

        assert_eq!(value, "100", "Define should have value=100");
    }

    #[pg_test]
    fn test_c_comment_documents_edge() {
        let source = r#"// Calculate the sum of two integers.
int add(int a, int b) {
    return a + b;
}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'comment_edge.c')",
            sql_escape(source),
        ))
        .unwrap();

        let doc_edge = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.edges e \
             JOIN kerai.nodes t ON e.target_id = t.id \
             WHERE e.relation = 'documents' \
             AND t.kind = 'c_function' AND t.content = 'add'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(doc_edge, 1, "Comment above add should create 'documents' edge");
    }

    #[pg_test]
    fn test_c_pointer_function() {
        let source = r#"int *foo(int x) {
    return &x;
}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'pointer_func.c')",
            sql_escape(source),
        ))
        .unwrap();

        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'c_function' AND content = 'foo'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(count, 1, "Should unwrap pointer declarator to find name 'foo'");
    }

    #[pg_test]
    fn test_c_reconstruct_roundtrip() {
        let source = r#"#include <stdio.h>

// A simple function
int add(int a, int b) {
    return a + b;
}
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'roundtrip.c')",
            sql_escape(source),
        ))
        .unwrap();

        let file_id = Spi::get_one::<String>(
            "SELECT id::text FROM kerai.nodes \
             WHERE kind = 'file' AND content = 'roundtrip.c' AND language = 'c'",
        )
        .unwrap()
        .unwrap();

        let reconstructed = Spi::get_one::<String>(&format!(
            "SELECT kerai.reconstruct_c_file('{}'::uuid)",
            sql_escape(&file_id),
        ))
        .unwrap()
        .unwrap();

        assert!(
            reconstructed.contains("#include"),
            "Reconstructed should contain include directive"
        );
        assert!(
            reconstructed.contains("int add"),
            "Reconstructed should contain add function"
        );
    }

    #[pg_test]
    fn test_c_typedef() {
        let source = r#"typedef struct {
    int x;
    int y;
} Point;
"#;
        Spi::run(&format!(
            "SELECT kerai.parse_c_source('{}', 'typedef_test.c')",
            sql_escape(source),
        ))
        .unwrap();

        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'c_typedef' AND content = 'Point'",
        )
        .unwrap()
        .unwrap_or(0);

        assert_eq!(count, 1, "Should have one c_typedef node named Point");
    }

    /// sql_escape helper for tests
    fn sql_escape(s: &str) -> String {
        s.replace('\'', "''")
    }

    // --- Plan 19: Repository ingestion tests ---

    /// Helper: create a temporary git repo with some files and a commit.
    fn create_test_repo(files: &[(&str, &[u8])]) -> (String, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().expect("Failed to create temp dir");
        let repo = git2::Repository::init(tmp.path()).expect("Failed to init repo");

        // Create files
        for (path, content) in files {
            let full_path = tmp.path().join(path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&full_path, content).expect("Failed to write file");
        }

        // Stage all files
        let mut index = repo.index().expect("Failed to get index");
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .expect("Failed to add files");
        index.write().expect("Failed to write index");
        let tree_oid = index.write_tree().expect("Failed to write tree");
        let tree = repo.find_tree(tree_oid).expect("Failed to find tree");

        // Create initial commit
        let sig = git2::Signature::now("Test Author", "test@test.com")
            .expect("Failed to create signature");
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .expect("Failed to create commit");

        let url = format!("file://{}", tmp.path().display());
        (url, tmp)
    }

    #[pg_test]
    fn test_mirror_repo_creates_nodes() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let (url, _tmp) = create_test_repo(&[
            ("hello.c", b"int main() { return 0; }"),
            ("README.md", b"# Hello\nWorld"),
        ]);

        let result = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .expect("mirror_repo query failed")
        .expect("mirror_repo returned NULL");

        let val = &result.0;
        assert_eq!(val["status"], "cloned");
        assert!(val["commits"].as_u64().unwrap() >= 1);
        assert!(val["files"].as_u64().unwrap() >= 2);

        // Verify repo_repository node exists
        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'repo_repository'",
        )
        .unwrap()
        .unwrap_or(0);
        assert!(count >= 1, "Expected at least 1 repo_repository node");
    }

    #[pg_test]
    fn test_commit_nodes_created() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let (url, _tmp) = create_test_repo(&[("file.txt", b"hello")]);

        Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .expect("mirror_repo failed")
        .expect("mirror_repo returned NULL");

        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'repo_commit'",
        )
        .unwrap()
        .unwrap_or(0);
        assert!(count >= 1, "Expected at least 1 commit node");

        // Verify commit metadata has sha
        let has_sha = Spi::get_one::<bool>(
            "SELECT (metadata->>'sha') IS NOT NULL FROM kerai.nodes WHERE kind = 'repo_commit' LIMIT 1",
        )
        .unwrap()
        .unwrap_or(false);
        assert!(has_sha, "Commit node should have sha in metadata");
    }

    #[pg_test]
    fn test_directory_nodes_created() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let (url, _tmp) = create_test_repo(&[
            ("src/main.c", b"int main() {}"),
            ("docs/README.md", b"# Docs"),
        ]);

        Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .expect("mirror_repo failed")
        .expect("mirror_repo returned NULL");

        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'repo_directory'",
        )
        .unwrap()
        .unwrap_or(0);
        assert!(count >= 2, "Expected at least 2 directory nodes (src, docs)");
    }

    #[pg_test]
    fn test_parsed_file_has_ast() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let c_source = b"int add(int a, int b) { return a + b; }\nvoid hello() {}\n";
        let (url, _tmp) = create_test_repo(&[("math.c", c_source)]);

        Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .expect("mirror_repo failed")
        .expect("mirror_repo returned NULL");

        // Should have c_function nodes from parsing
        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'c_function'",
        )
        .unwrap()
        .unwrap_or(0);
        assert!(count >= 1, "Expected c_function nodes from parsed C file");
    }

    #[pg_test]
    fn test_opaque_text_file() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let (url, _tmp) = create_test_repo(&[
            ("script.py", b"print('hello world')\nx = 42\n"),
        ]);

        Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .expect("mirror_repo failed")
        .expect("mirror_repo returned NULL");

        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'repo_opaque_text'",
        )
        .unwrap()
        .unwrap_or(0);
        assert!(count >= 1, "Expected opaque_text node for .py file");

        // Verify source is in metadata
        let has_source = Spi::get_one::<bool>(
            "SELECT (metadata->>'source') IS NOT NULL FROM kerai.nodes WHERE kind = 'repo_opaque_text' LIMIT 1",
        )
        .unwrap()
        .unwrap_or(false);
        assert!(has_source, "Opaque text node should have source in metadata");
    }

    #[pg_test]
    fn test_opaque_binary_file() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        // Create a file with null bytes to trigger binary detection
        let binary_content: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0x00, 0x00];
        let (url, _tmp) = create_test_repo(&[("image.png", &binary_content)]);

        Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .expect("mirror_repo failed")
        .expect("mirror_repo returned NULL");

        let count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'repo_opaque_binary'",
        )
        .unwrap()
        .unwrap_or(0);
        assert!(count >= 1, "Expected opaque_binary node for .png file");

        // Verify sha256 in metadata
        let has_hash = Spi::get_one::<bool>(
            "SELECT (metadata->>'sha256') IS NOT NULL FROM kerai.nodes WHERE kind = 'repo_opaque_binary' LIMIT 1",
        )
        .unwrap()
        .unwrap_or(false);
        assert!(has_hash, "Binary node should have sha256 in metadata");
    }

    #[pg_test]
    fn test_repo_census() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let (url, _tmp) = create_test_repo(&[
            ("main.c", b"int main() {}"),
            ("lib.c", b"void lib() {}"),
            ("script.py", b"print('hello')"),
            ("README.md", b"# Readme"),
        ]);

        Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .expect("mirror_repo failed")
        .expect("mirror_repo returned NULL");

        let census = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.repo_census((SELECT id FROM kerai.repositories LIMIT 1))",
        )
        .expect("census query failed")
        .expect("census returned NULL");

        let val = &census.0;
        assert!(val["total_files"].as_i64().unwrap() >= 3);
        assert!(val["languages"].is_object());
    }

    #[pg_test]
    fn test_mirror_idempotent() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let (url, _tmp) = create_test_repo(&[("file.txt", b"hello")]);

        // First mirror
        let r1 = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .unwrap()
        .unwrap();
        assert_eq!(r1.0["status"], "cloned");

        // Second mirror — should be up_to_date
        let r2 = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .unwrap()
        .unwrap();
        assert_eq!(r2.0["status"], "up_to_date");
    }

    #[pg_test]
    fn test_incremental_update() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = git2::Repository::init(tmp.path()).expect("init");
        let sig = git2::Signature::now("Test", "t@t.com").expect("sig");

        // Initial commit
        std::fs::write(tmp.path().join("file.txt"), b"hello").expect("write");
        let mut index = repo.index().expect("index");
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None).expect("add");
        index.write().expect("write idx");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let c1 = repo.commit(Some("HEAD"), &sig, &sig, "First", &tree, &[]).expect("commit");

        let url = format!("file://{}", tmp.path().display());

        // First mirror
        let r1 = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .unwrap()
        .unwrap();
        assert_eq!(r1.0["status"], "cloned");

        let commits_before = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'repo_commit'",
        )
        .unwrap()
        .unwrap_or(0);

        // Add a second commit
        std::fs::write(tmp.path().join("new.txt"), b"world").expect("write");
        let mut index2 = repo.index().expect("index");
        index2.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None).expect("add");
        index2.write().expect("write idx");
        let tree_oid2 = index2.write_tree().expect("write tree");
        let tree2 = repo.find_tree(tree_oid2).expect("find tree");
        let parent = repo.find_commit(c1).expect("find parent");
        repo.commit(Some("HEAD"), &sig, &sig, "Second", &tree2, &[&parent]).expect("commit");

        // Second mirror — should pick up new commit
        let r2 = Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .unwrap()
        .unwrap();
        assert_eq!(r2.0["status"], "updated");
        assert!(r2.0["commits"].as_u64().unwrap() >= 1);
    }

    #[pg_test]
    fn test_drop_repo() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let (url, _tmp) = create_test_repo(&[("file.c", b"int x;")]);

        Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .unwrap()
        .unwrap();

        // Verify nodes exist
        let before = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'repo_repository'",
        )
        .unwrap()
        .unwrap_or(0);
        assert!(before >= 1);

        // Drop
        let drop_result = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.drop_repo((SELECT id FROM kerai.repositories LIMIT 1))",
        )
        .unwrap()
        .unwrap();
        assert_eq!(drop_result.0["dropped"], true);

        // Verify nodes cleaned up
        let after = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.nodes WHERE kind = 'repo_repository'",
        )
        .unwrap()
        .unwrap_or(0);
        assert_eq!(after, 0);

        // Verify repository record cleaned up
        let repo_count = Spi::get_one::<i64>(
            "SELECT count(*) FROM kerai.repositories",
        )
        .unwrap()
        .unwrap_or(0);
        assert_eq!(repo_count, 0);
    }

    #[pg_test]
    fn test_list_repos() {
        Spi::run("SELECT kerai.bootstrap_instance()").ok();

        let (url, _tmp) = create_test_repo(&[("file.txt", b"hello")]);

        Spi::get_one::<pgrx::JsonB>(&format!(
            "SELECT kerai.mirror_repo('{}')",
            sql_escape(&url),
        ))
        .unwrap()
        .unwrap();

        let list = Spi::get_one::<pgrx::JsonB>(
            "SELECT kerai.list_repos()",
        )
        .unwrap()
        .unwrap();

        let repos = list.0.as_array().expect("list_repos should return array");
        assert!(!repos.is_empty(), "Should have at least one repo");
        assert!(repos[0]["url"].as_str().is_some());
        assert!(repos[0]["name"].as_str().is_some());
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
