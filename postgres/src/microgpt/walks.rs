use pgrx::prelude::*;
use rand::seq::SliceRandom;
use rand::Rng;

/// Build vocabulary: assign dense integer indices to nodes.
/// Returns the vocab size.
pub fn build_vocab(agent_id: &str, scope: Option<&str>) -> Result<usize, String> {
    // Clear existing vocab for this model
    let clear_sql = format!(
        "DELETE FROM kerai.model_vocab WHERE model_id = '{agent_id}'::uuid"
    );
    Spi::run(&clear_sql).map_err(|e| format!("Failed to clear vocab: {e}"))?;

    // Select nodes, optionally scoped by ltree
    let select_sql = match scope {
        Some(s) => format!(
            "SELECT id::text FROM kerai.nodes WHERE path <@ '{}'::ltree ORDER BY path, position",
            s.replace('\'', "''")
        ),
        None => "SELECT id::text FROM kerai.nodes ORDER BY path, position".to_string(),
    };

    let mut node_ids: Vec<String> = Vec::new();
    Spi::connect(|client| {
        let tup_table = client
            .select(&select_sql, None, &[])
            .map_err(|e| format!("SPI error: {e}"))?;
        for row in tup_table {
            if let Ok(Some(id)) = row.get_by_name::<String, _>("id") {
                node_ids.push(id);
            }
        }
        Ok::<(), String>(())
    })?;

    if node_ids.is_empty() {
        return Ok(0);
    }

    // Batch insert vocab entries
    let batch_size = 500;
    for chunk in node_ids.chunks(batch_size) {
        let values: String = chunk
            .iter()
            .enumerate()
            .map(|(offset, id)| {
                let idx = node_ids
                    .iter()
                    .position(|x| x == id)
                    .unwrap_or(offset);
                format!("('{agent_id}'::uuid, '{id}'::uuid, {idx})")
            })
            .collect::<Vec<_>>()
            .join(",");
        let insert_sql = format!(
            "INSERT INTO kerai.model_vocab (model_id, node_id, token_idx) VALUES {values}"
        );
        Spi::run(&insert_sql).map_err(|e| format!("Failed to insert vocab: {e}"))?;
    }

    Ok(node_ids.len())
}

/// Map node UUIDs to token indices.
pub fn uuids_to_indices(agent_id: &str, uuids: &[String]) -> Result<Vec<usize>, String> {
    if uuids.is_empty() {
        return Ok(Vec::new());
    }
    let uuid_list: String = uuids
        .iter()
        .map(|u| format!("'{}'::uuid", u.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT node_id::text, token_idx FROM kerai.model_vocab
         WHERE model_id = '{agent_id}'::uuid AND node_id IN ({uuid_list})
         ORDER BY array_position(ARRAY[{uuid_list}], node_id)"
    );

    let mut indices = Vec::new();
    Spi::connect(|client| {
        let tup_table = client
            .select(&sql, None, &[])
            .map_err(|e| format!("SPI error: {e}"))?;
        for row in tup_table {
            if let Ok(Some(idx)) = row.get_by_name::<i32, _>("token_idx") {
                indices.push(idx as usize);
            }
        }
        Ok::<(), String>(())
    })?;

    Ok(indices)
}

/// Map (token_index, probability) pairs back to (UUID, probability).
pub fn indices_to_uuids(
    agent_id: &str,
    predictions: &[(usize, f32)],
) -> Result<Vec<(String, f64)>, String> {
    if predictions.is_empty() {
        return Ok(Vec::new());
    }
    let idx_list: String = predictions
        .iter()
        .map(|(idx, _)| idx.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT token_idx, node_id::text FROM kerai.model_vocab
         WHERE model_id = '{agent_id}'::uuid AND token_idx IN ({idx_list})"
    );

    let mut idx_to_uuid = std::collections::HashMap::new();
    Spi::connect(|client| {
        let tup_table = client
            .select(&sql, None, &[])
            .map_err(|e| format!("SPI error: {e}"))?;
        for row in tup_table {
            let idx: i32 = row
                .get_by_name::<i32, _>("token_idx")
                .map_err(|e| format!("column error: {e}"))?
                .ok_or_else(|| "null token_idx".to_string())?;
            let uuid: String = row
                .get_by_name::<String, _>("node_id")
                .map_err(|e| format!("column error: {e}"))?
                .ok_or_else(|| "null node_id".to_string())?;
            idx_to_uuid.insert(idx as usize, uuid);
        }
        Ok::<(), String>(())
    })?;

    Ok(predictions
        .iter()
        .filter_map(|(idx, prob)| {
            idx_to_uuid
                .get(idx)
                .map(|uuid| (uuid.clone(), *prob as f64))
        })
        .collect())
}

/// Generate walk sequences over the graph.
///
/// walk_type: "tree", "edge", "perspective", "random"
/// Returns Vec of token-index sequences.
pub fn generate_walks(
    agent_id: &str,
    walk_type: &str,
    n_sequences: usize,
    context_len: usize,
    scope: Option<&str>,
    perspective_agent: Option<&str>,
) -> Result<Vec<Vec<usize>>, String> {
    match walk_type {
        "tree" => generate_tree_walks(agent_id, n_sequences, context_len, scope),
        "edge" => generate_edge_walks(agent_id, n_sequences, context_len, scope),
        "perspective" => {
            generate_perspective_walks(agent_id, n_sequences, context_len, scope, perspective_agent)
        }
        "random" => generate_random_walks(agent_id, n_sequences, context_len, scope),
        _ => Err(format!("Unknown walk type: {}", walk_type)),
    }
}

/// Tree walk: depth-first parent→child traversal ordered by position.
fn generate_tree_walks(
    agent_id: &str,
    n_sequences: usize,
    context_len: usize,
    scope: Option<&str>,
) -> Result<Vec<Vec<usize>>, String> {
    // Get root nodes (nodes with no parent, or scoped roots)
    let roots_sql = match scope {
        Some(s) => format!(
            "SELECT v.token_idx FROM kerai.model_vocab v
             JOIN kerai.nodes n ON n.id = v.node_id
             WHERE v.model_id = '{agent_id}'::uuid
               AND n.path <@ '{}'::ltree
               AND n.parent_id IS NULL
             ORDER BY n.position",
            s.replace('\'', "''")
        ),
        None => format!(
            "SELECT v.token_idx FROM kerai.model_vocab v
             JOIN kerai.nodes n ON n.id = v.node_id
             WHERE v.model_id = '{agent_id}'::uuid
               AND n.parent_id IS NULL
             ORDER BY n.position"
        ),
    };

    let mut root_indices: Vec<usize> = Vec::new();
    Spi::connect(|client| {
        let tup_table = client
            .select(&roots_sql, None, &[])
            .map_err(|e| format!("SPI error: {e}"))?;
        for row in tup_table {
            if let Ok(Some(idx)) = row.get_by_name::<i32, _>("token_idx") {
                root_indices.push(idx as usize);
            }
        }
        Ok::<(), String>(())
    })?;

    if root_indices.is_empty() {
        return Ok(Vec::new());
    }

    // Build parent→children adjacency from vocab
    let children_sql = format!(
        "SELECT pv.token_idx AS parent_idx, cv.token_idx AS child_idx
         FROM kerai.model_vocab cv
         JOIN kerai.nodes cn ON cn.id = cv.node_id
         JOIN kerai.model_vocab pv ON pv.node_id = cn.parent_id AND pv.model_id = cv.model_id
         WHERE cv.model_id = '{agent_id}'::uuid
         ORDER BY cn.position"
    );

    let mut children_map: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    Spi::connect(|client| {
        let tup_table = client
            .select(&children_sql, None, &[])
            .map_err(|e| format!("SPI error: {e}"))?;
        for row in tup_table {
            let parent: i32 = row.get_by_name::<i32, _>("parent_idx").ok().flatten().unwrap_or(-1);
            let child: i32 = row.get_by_name::<i32, _>("child_idx").ok().flatten().unwrap_or(-1);
            if parent >= 0 && child >= 0 {
                children_map
                    .entry(parent as usize)
                    .or_default()
                    .push(child as usize);
            }
        }
        Ok::<(), String>(())
    })?;

    // DFS from each root to generate sequences
    let mut sequences = Vec::new();
    let mut rng = rand::thread_rng();

    for _ in 0..n_sequences {
        let root = root_indices[rng.gen_range(0..root_indices.len())];
        let mut seq = Vec::with_capacity(context_len);
        let mut stack = vec![root];

        while let Some(node) = stack.pop() {
            if seq.len() >= context_len {
                break;
            }
            seq.push(node);
            if let Some(children) = children_map.get(&node) {
                // Push in reverse order so first child is popped first
                for &child in children.iter().rev() {
                    stack.push(child);
                }
            }
        }

        if seq.len() >= 2 {
            sequences.push(seq);
        }
    }

    Ok(sequences)
}

/// Edge walk: follow edges from each start node N hops deep.
fn generate_edge_walks(
    agent_id: &str,
    n_sequences: usize,
    context_len: usize,
    scope: Option<&str>,
) -> Result<Vec<Vec<usize>>, String> {
    // Build edge adjacency
    let edge_sql = match scope {
        Some(s) => format!(
            "SELECT sv.token_idx AS src_idx, tv.token_idx AS tgt_idx
             FROM kerai.edges e
             JOIN kerai.model_vocab sv ON sv.node_id = e.source_id AND sv.model_id = '{agent_id}'::uuid
             JOIN kerai.model_vocab tv ON tv.node_id = e.target_id AND tv.model_id = '{agent_id}'::uuid
             JOIN kerai.nodes sn ON sn.id = e.source_id
             WHERE sn.path <@ '{}'::ltree",
            s.replace('\'', "''")
        ),
        None => format!(
            "SELECT sv.token_idx AS src_idx, tv.token_idx AS tgt_idx
             FROM kerai.edges e
             JOIN kerai.model_vocab sv ON sv.node_id = e.source_id AND sv.model_id = '{agent_id}'::uuid
             JOIN kerai.model_vocab tv ON tv.node_id = e.target_id AND tv.model_id = '{agent_id}'::uuid"
        ),
    };

    let mut adj: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    let mut all_nodes: Vec<usize> = Vec::new();

    Spi::connect(|client| {
        let tup_table = client
            .select(&edge_sql, None, &[])
            .map_err(|e| format!("SPI error: {e}"))?;
        for row in tup_table {
            let src: i32 = row.get_by_name::<i32, _>("src_idx").ok().flatten().unwrap_or(-1);
            let tgt: i32 = row.get_by_name::<i32, _>("tgt_idx").ok().flatten().unwrap_or(-1);
            if src >= 0 && tgt >= 0 {
                adj.entry(src as usize).or_default().push(tgt as usize);
                if !all_nodes.contains(&(src as usize)) {
                    all_nodes.push(src as usize);
                }
            }
        }
        Ok::<(), String>(())
    })?;

    if all_nodes.is_empty() {
        // Fall back to tree walks if no edges exist
        return generate_tree_walks(agent_id, n_sequences, context_len, scope);
    }

    let mut rng = rand::thread_rng();
    let mut sequences = Vec::new();

    for _ in 0..n_sequences {
        let start = all_nodes[rng.gen_range(0..all_nodes.len())];
        let mut seq = vec![start];
        let mut current = start;

        for _ in 1..context_len {
            if let Some(neighbors) = adj.get(&current) {
                if neighbors.is_empty() {
                    break;
                }
                current = neighbors[rng.gen_range(0..neighbors.len())];
                seq.push(current);
            } else {
                break;
            }
        }

        if seq.len() >= 2 {
            sequences.push(seq);
        }
    }

    Ok(sequences)
}

/// Perspective walk: random walk weighted by perspective weights.
fn generate_perspective_walks(
    agent_id: &str,
    n_sequences: usize,
    context_len: usize,
    scope: Option<&str>,
    perspective_agent: Option<&str>,
) -> Result<Vec<Vec<usize>>, String> {
    // Get perspective agent ID
    let persp_agent_id = match perspective_agent {
        Some(name) => {
            let sql = format!(
                "SELECT id::text FROM kerai.agents WHERE name = '{}'",
                name.replace('\'', "''")
            );
            Spi::get_one::<String>(&sql)
                .map_err(|e| format!("SPI error: {e}"))?
                .unwrap_or_else(|| agent_id.to_string())
        }
        None => agent_id.to_string(),
    };

    // Build adjacency with perspective weights
    let scope_filter = match scope {
        Some(s) => format!("AND sn.path <@ '{}'::ltree", s.replace('\'', "''")),
        None => String::new(),
    };

    let adj_sql = format!(
        "SELECT sv.token_idx AS src_idx, tv.token_idx AS tgt_idx,
                COALESCE(p.weight, 0.0) AS weight
         FROM kerai.edges e
         JOIN kerai.model_vocab sv ON sv.node_id = e.source_id AND sv.model_id = '{agent_id}'::uuid
         JOIN kerai.model_vocab tv ON tv.node_id = e.target_id AND tv.model_id = '{agent_id}'::uuid
         JOIN kerai.nodes sn ON sn.id = e.source_id
         LEFT JOIN kerai.perspectives p ON p.node_id = e.target_id AND p.agent_id = '{persp_agent_id}'::uuid
         WHERE 1=1 {scope_filter}"
    );

    let mut adj: std::collections::HashMap<usize, Vec<(usize, f64)>> =
        std::collections::HashMap::new();
    let mut all_nodes: Vec<usize> = Vec::new();

    Spi::connect(|client| {
        let tup_table = client
            .select(&adj_sql, None, &[])
            .map_err(|e| format!("SPI error: {e}"))?;
        for row in tup_table {
            let src: i32 = row.get_by_name::<i32, _>("src_idx").ok().flatten().unwrap_or(-1);
            let tgt: i32 = row.get_by_name::<i32, _>("tgt_idx").ok().flatten().unwrap_or(-1);
            let weight: f64 = row.get_by_name::<f64, _>("weight").ok().flatten().unwrap_or(0.0);
            if src >= 0 && tgt >= 0 {
                adj.entry(src as usize)
                    .or_default()
                    .push((tgt as usize, (1.0 + weight.abs()).max(0.01)));
                if !all_nodes.contains(&(src as usize)) {
                    all_nodes.push(src as usize);
                }
            }
        }
        Ok::<(), String>(())
    })?;

    if all_nodes.is_empty() {
        return generate_tree_walks(agent_id, n_sequences, context_len, scope);
    }

    let mut rng = rand::thread_rng();
    let mut sequences = Vec::new();

    for _ in 0..n_sequences {
        let start = all_nodes[rng.gen_range(0..all_nodes.len())];
        let mut seq = vec![start];
        let mut current = start;

        for _ in 1..context_len {
            if let Some(neighbors) = adj.get(&current) {
                if neighbors.is_empty() {
                    break;
                }
                // Weighted random selection
                let total_weight: f64 = neighbors.iter().map(|(_, w)| w).sum();
                let mut r = rng.gen::<f64>() * total_weight;
                let mut chosen = neighbors[0].0;
                for &(idx, w) in neighbors {
                    r -= w;
                    if r <= 0.0 {
                        chosen = idx;
                        break;
                    }
                }
                current = chosen;
                seq.push(current);
            } else {
                break;
            }
        }

        if seq.len() >= 2 {
            sequences.push(seq);
        }
    }

    Ok(sequences)
}

/// Random walk: uniform random traversal over edges.
fn generate_random_walks(
    agent_id: &str,
    n_sequences: usize,
    context_len: usize,
    scope: Option<&str>,
) -> Result<Vec<Vec<usize>>, String> {
    // Combine parent-child and edge adjacency for maximum connectivity
    let scope_filter = match scope {
        Some(s) => format!("AND n.path <@ '{}'::ltree", s.replace('\'', "''")),
        None => String::new(),
    };

    // Parent→child edges
    let tree_sql = format!(
        "SELECT pv.token_idx AS src_idx, cv.token_idx AS tgt_idx
         FROM kerai.model_vocab cv
         JOIN kerai.nodes cn ON cn.id = cv.node_id
         JOIN kerai.model_vocab pv ON pv.node_id = cn.parent_id AND pv.model_id = cv.model_id
         JOIN kerai.nodes n ON n.id = cn.id
         WHERE cv.model_id = '{agent_id}'::uuid {scope_filter}"
    );

    // Explicit edges
    let edge_sql = format!(
        "SELECT sv.token_idx AS src_idx, tv.token_idx AS tgt_idx
         FROM kerai.edges e
         JOIN kerai.model_vocab sv ON sv.node_id = e.source_id AND sv.model_id = '{agent_id}'::uuid
         JOIN kerai.model_vocab tv ON tv.node_id = e.target_id AND tv.model_id = '{agent_id}'::uuid"
    );

    let mut adj: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    let mut all_nodes: Vec<usize> = Vec::new();

    // Collect from both sources
    Spi::connect(|client| {
        for sql in [&tree_sql, &edge_sql] {
            let tup_table = client
                .select(sql.as_str(), None, &[])
                .map_err(|e| format!("SPI error: {e}"))?;
            for row in tup_table {
                let src: i32 = row.get_by_name::<i32, _>("src_idx").ok().flatten().unwrap_or(-1);
                let tgt: i32 = row.get_by_name::<i32, _>("tgt_idx").ok().flatten().unwrap_or(-1);
                if src >= 0 && tgt >= 0 {
                    adj.entry(src as usize).or_default().push(tgt as usize);
                    // Also add reverse direction for random walks
                    adj.entry(tgt as usize).or_default().push(src as usize);
                    if !all_nodes.contains(&(src as usize)) {
                        all_nodes.push(src as usize);
                    }
                    if !all_nodes.contains(&(tgt as usize)) {
                        all_nodes.push(tgt as usize);
                    }
                }
            }
        }
        Ok::<(), String>(())
    })?;

    if all_nodes.is_empty() {
        return Ok(Vec::new());
    }

    let mut rng = rand::thread_rng();
    let mut sequences = Vec::new();

    for _ in 0..n_sequences {
        let start = all_nodes[rng.gen_range(0..all_nodes.len())];
        let mut seq = vec![start];
        let mut current = start;

        for _ in 1..context_len {
            if let Some(neighbors) = adj.get(&current) {
                if neighbors.is_empty() {
                    break;
                }
                current = *neighbors.choose(&mut rng).unwrap();
                seq.push(current);
            } else {
                break;
            }
        }

        if seq.len() >= 2 {
            sequences.push(seq);
        }
    }

    Ok(sequences)
}
