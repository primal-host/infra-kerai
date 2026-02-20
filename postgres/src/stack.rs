use pgrx::prelude::*;

use crate::sql::sql_text;

const SELF_INSTANCE: &str =
    "(SELECT id FROM kerai.instances WHERE is_self = true)";

/// Push content onto the stack. Returns the new position.
#[pg_extern]
fn stack_push(content: &str, label: Option<&str>) -> i32 {
    let pos = Spi::get_one::<i32>(&format!(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM kerai.stack \
         WHERE instance_id = {SELF_INSTANCE}",
    ))
    .unwrap()
    .unwrap_or(0);

    let label_sql = match label {
        Some(l) => sql_text(l),
        None => "NULL".to_string(),
    };

    Spi::run(&format!(
        "INSERT INTO kerai.stack (instance_id, position, label, content) \
         VALUES ({SELF_INSTANCE}, {pos}, {label_sql}, {})",
        sql_text(content),
    ))
    .expect("stack_push SPI failed");

    pos
}

/// Return the content at the top of the stack without removing it.
#[pg_extern]
fn stack_peek() -> Option<String> {
    Spi::get_one::<String>(&format!(
        "SELECT content FROM kerai.stack \
         WHERE instance_id = {SELF_INSTANCE} \
         ORDER BY position DESC LIMIT 1",
    ))
    .unwrap_or(None)
}

/// Pop the top entry: return its content and delete it.
#[pg_extern]
fn stack_pop() -> Option<String> {
    Spi::get_one::<String>(&format!(
        "WITH top AS ( \
             DELETE FROM kerai.stack \
             WHERE id = ( \
                 SELECT id FROM kerai.stack \
                 WHERE instance_id = {SELF_INSTANCE} \
                 ORDER BY position DESC LIMIT 1 \
             ) \
             RETURNING content \
         ) SELECT content FROM top",
    ))
    .unwrap_or(None)
}

/// Drop (delete) the top entry. Returns "dropped" or "empty".
#[pg_extern]
fn stack_drop() -> &'static str {
    let deleted = Spi::get_one::<bool>(&format!(
        "WITH d AS ( \
             DELETE FROM kerai.stack \
             WHERE id = ( \
                 SELECT id FROM kerai.stack \
                 WHERE instance_id = {SELF_INSTANCE} \
                 ORDER BY position DESC LIMIT 1 \
             ) \
             RETURNING id \
         ) SELECT EXISTS(SELECT 1 FROM d)",
    ))
    .unwrap_or(Some(false))
    .unwrap_or(false);

    if deleted { "dropped" } else { "empty" }
}

/// Replace the content of the top stack entry (for editing in place).
#[pg_extern]
fn stack_replace(content: &str) -> &'static str {
    let updated = Spi::get_one::<bool>(&format!(
        "WITH u AS ( \
             UPDATE kerai.stack SET content = {} \
             WHERE id = ( \
                 SELECT id FROM kerai.stack \
                 WHERE instance_id = {SELF_INSTANCE} \
                 ORDER BY position DESC LIMIT 1 \
             ) \
             RETURNING id \
         ) SELECT EXISTS(SELECT 1 FROM u)",
        sql_text(content),
    ))
    .unwrap_or(Some(false))
    .unwrap_or(false);

    if updated { "replaced" } else { "empty" }
}

/// List all stack entries (position, label, preview, created_at).
#[pg_extern]
fn stack_list() -> TableIterator<
    'static,
    (
        name!(position, i32),
        name!(label, String),
        name!(preview, String),
        name!(created_at, String),
    ),
> {
    let query = format!(
        "SELECT position, COALESCE(label, ''), \
         LEFT(REPLACE(content, E'\\n', ' '), 60), \
         created_at::text \
         FROM kerai.stack \
         WHERE instance_id = {SELF_INSTANCE} \
         ORDER BY position DESC",
    );

    let mut rows = Vec::new();
    Spi::connect(|client| {
        let tup_table = client.select(&query, None, &[]).unwrap();
        for row in tup_table {
            let position: i32 = row.get_by_name("position").unwrap().unwrap_or(0);
            let label: String = row.get_by_name("label").unwrap().unwrap_or_default();
            let preview: String = row.get_by_name("left").unwrap().unwrap_or_default();
            let created_at: String = row.get_by_name("created_at").unwrap().unwrap_or_default();
            rows.push((position, label, preview, created_at));
        }
    });

    TableIterator::new(rows)
}

/// Return the number of entries on the stack.
#[pg_extern]
fn stack_depth() -> i32 {
    Spi::get_one::<i64>(&format!(
        "SELECT COUNT(*) FROM kerai.stack WHERE instance_id = {SELF_INSTANCE}",
    ))
    .unwrap()
    .unwrap_or(0) as i32
}

/// Clear all stack entries. Returns the number deleted.
#[pg_extern]
fn stack_clear() -> i32 {
    // SPI DELETE doesn't directly return row count via get_one,
    // so we use a CTE to count.
    let count = Spi::get_one::<i64>(&format!(
        "WITH d AS ( \
             DELETE FROM kerai.stack \
             WHERE instance_id = {SELF_INSTANCE} \
             RETURNING id \
         ) SELECT COUNT(*) FROM d",
    ))
    .unwrap()
    .unwrap_or(0);

    count as i32
}
