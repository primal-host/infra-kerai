pub mod model;
pub mod optimizer;
pub mod tensor;
pub mod walks;

use pgrx::prelude::*;

use self::model::{MicroGPT, ModelConfig};
use self::tensor::Tensor;

/// Helper: look up agent_id by name.
fn agent_id_by_name(agent_name: &str) -> Result<String, String> {
    let sql = format!(
        "SELECT id::text FROM kerai.agents WHERE name = '{}'",
        agent_name.replace('\'', "''")
    );
    Spi::get_one::<String>(&sql)
        .map_err(|e| format!("SPI error: {e}"))?
        .ok_or_else(|| format!("Agent '{}' not found", agent_name))
}

/// Helper: store model weights to DB.
fn store_weights(agent_id: &str, model: &MicroGPT) -> Result<(), String> {
    let weight_map = model.to_weight_map();
    for (name, tensor) in &weight_map {
        let bytes = tensor.to_bytes();
        let hex = bytes_to_pg_hex(&bytes);
        let shape_sql: String = tensor
            .shape
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "INSERT INTO kerai.model_weights (agent_id, tensor_name, tensor_data, shape)
             VALUES ('{agent_id}'::uuid, '{name}', '{hex}'::bytea, ARRAY[{shape_sql}]::integer[])
             ON CONFLICT (agent_id, tensor_name)
             DO UPDATE SET tensor_data = EXCLUDED.tensor_data, shape = EXCLUDED.shape,
                           version = kerai.model_weights.version + 1,
                           updated_at = now()"
        );
        Spi::run(&sql).map_err(|e| format!("Failed to store weight '{}': {}", name, e))?;
    }
    Ok(())
}

/// Helper: load model weights from DB.
fn load_weights(agent_id: &str, config: &ModelConfig) -> Result<MicroGPT, String> {
    let mut weight_map = std::collections::HashMap::new();

    let sql = format!(
        "SELECT tensor_name, tensor_data, shape FROM kerai.model_weights WHERE agent_id = '{agent_id}'::uuid"
    );
    Spi::connect(|client| {
        let tup_table = client.select(&sql, None, &[])
            .map_err(|e| format!("SPI error: {e}"))?;
        for row in tup_table {
            let name: String = row.get_by_name::<String, _>("tensor_name")
                .map_err(|e| format!("column error: {e}"))?
                .ok_or_else(|| "null tensor_name".to_string())?;
            let data_bytes: Vec<u8> = row.get_by_name::<Vec<u8>, _>("tensor_data")
                .map_err(|e| format!("column error: {e}"))?
                .ok_or_else(|| "null tensor_data".to_string())?;
            let shape_i32: Vec<i32> = row.get_by_name::<Vec<i32>, _>("shape")
                .map_err(|e| format!("column error: {e}"))?
                .ok_or_else(|| "null shape".to_string())?;
            let shape: Vec<usize> = shape_i32.iter().map(|&s| s as usize).collect();
            let tensor = Tensor::from_bytes(&data_bytes, shape);
            weight_map.insert(name, tensor);
        }
        Ok::<(), String>(())
    })?;

    if weight_map.is_empty() {
        return Err(format!("No weights found for agent '{}'", agent_id));
    }

    Ok(MicroGPT::from_weight_map(config.clone(), &weight_map))
}

/// Helper: load model config from agent's config JSONB.
fn load_model_config(agent_id: &str) -> Result<ModelConfig, String> {
    let sql = format!(
        "SELECT config::text FROM kerai.agents WHERE id = '{agent_id}'::uuid"
    );
    let config_str = Spi::get_one::<String>(&sql)
        .map_err(|e| format!("SPI error: {e}"))?
        .ok_or_else(|| "No config found".to_string())?;
    let config_json: serde_json::Value =
        serde_json::from_str(&config_str).map_err(|e| format!("JSON parse error: {e}"))?;

    Ok(ModelConfig {
        vocab_size: config_json
            .get("vocab_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as usize,
        dim: config_json
            .get("dim")
            .and_then(|v| v.as_u64())
            .unwrap_or(32) as usize,
        n_heads: config_json
            .get("n_heads")
            .and_then(|v| v.as_u64())
            .unwrap_or(4) as usize,
        n_layers: config_json
            .get("n_layers")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize,
        context_len: config_json
            .get("context_len")
            .and_then(|v| v.as_u64())
            .unwrap_or(16) as usize,
    })
}

fn bytes_to_pg_hex(bytes: &[u8]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("\\x{}", hex)
}

/// Create a new MicroGPT model for an agent.
/// Builds vocabulary from graph nodes, initializes random weights, stores to DB.
#[pg_extern]
fn create_model(
    agent_name: &str,
    dim: default!(Option<i32>, "NULL"),
    n_heads: default!(Option<i32>, "NULL"),
    n_layers: default!(Option<i32>, "NULL"),
    context_len: default!(Option<i32>, "NULL"),
    scope: default!(Option<&str>, "NULL"),
) -> pgrx::JsonB {
    let agent_id = agent_id_by_name(agent_name).unwrap_or_else(|e| error!("{e}"));

    // Build vocabulary
    let vocab_size = walks::build_vocab(&agent_id, scope)
        .unwrap_or_else(|e| error!("Failed to build vocab: {e}"));

    if vocab_size == 0 {
        error!("No nodes found to build vocabulary");
    }

    let config = ModelConfig {
        vocab_size,
        dim: dim.unwrap_or(32) as usize,
        n_heads: n_heads.unwrap_or(4) as usize,
        n_layers: n_layers.unwrap_or(1) as usize,
        context_len: context_len.unwrap_or(16) as usize,
    };

    // Validate config
    if config.dim % config.n_heads != 0 {
        error!(
            "dim ({}) must be divisible by n_heads ({})",
            config.dim, config.n_heads
        );
    }

    // Store config in agent's config column
    let config_json = serde_json::json!({
        "vocab_size": config.vocab_size,
        "dim": config.dim,
        "n_heads": config.n_heads,
        "n_layers": config.n_layers,
        "context_len": config.context_len,
    });
    let config_sql = format!(
        "UPDATE kerai.agents SET config = '{}'::jsonb WHERE id = '{}'::uuid",
        config_json, agent_id
    );
    Spi::run(&config_sql).unwrap_or_else(|e| error!("Failed to update agent config: {e}"));

    // Initialize model with random weights
    let model = MicroGPT::new(config.clone());
    let param_count = model.param_count();
    let param_bytes = param_count * 4;

    // Store weights
    store_weights(&agent_id, &model).unwrap_or_else(|e| error!("{e}"));

    pgrx::JsonB(serde_json::json!({
        "status": "created",
        "agent": agent_name,
        "vocab_size": config.vocab_size,
        "dim": config.dim,
        "n_heads": config.n_heads,
        "n_layers": config.n_layers,
        "context_len": config.context_len,
        "param_count": param_count,
        "param_bytes": param_bytes,
    }))
}

/// Train a model on graph walk sequences.
#[pg_extern]
fn train_model(
    agent_name: &str,
    walk_type: default!(Option<&str>, "'tree'"),
    n_sequences: default!(Option<i32>, "NULL"),
    n_steps: default!(Option<i32>, "NULL"),
    lr: default!(Option<f64>, "NULL"),
    scope: default!(Option<&str>, "NULL"),
    perspective_agent: default!(Option<&str>, "NULL"),
) -> pgrx::JsonB {
    let start = std::time::Instant::now();
    let agent_id = agent_id_by_name(agent_name).unwrap_or_else(|e| error!("{e}"));
    let config = load_model_config(&agent_id).unwrap_or_else(|e| error!("{e}"));
    let mut model = load_weights(&agent_id, &config).unwrap_or_else(|e| error!("{e}"));

    let walk = walk_type.unwrap_or("tree");
    let n_seq = n_sequences.unwrap_or(50) as usize;
    let steps = n_steps.unwrap_or(100) as usize;
    let learning_rate = lr.unwrap_or(0.001) as f32;

    // Generate walk sequences
    let sequences = walks::generate_walks(
        &agent_id,
        walk,
        n_seq,
        config.context_len,
        scope,
        perspective_agent,
    )
    .unwrap_or_else(|e| error!("Failed to generate walks: {e}"));

    if sequences.is_empty() {
        error!("No walk sequences generated — not enough connected nodes");
    }

    // Training loop
    let mut optimizer = optimizer::Adam::new(model.param_count(), learning_rate);
    let mut losses = Vec::with_capacity(steps);
    let batch_size = 8.min(sequences.len());

    for step in 0..steps {
        // Sample a batch
        let batch: Vec<Vec<usize>> = {
            use rand::seq::SliceRandom;
            let mut rng = rand::thread_rng();
            let mut indices: Vec<usize> = (0..sequences.len()).collect();
            indices.shuffle(&mut rng);
            indices
                .iter()
                .take(batch_size)
                .map(|&i| sequences[i].clone())
                .collect()
        };

        let loss = model.train_step(&batch, &mut optimizer);
        losses.push(loss);

        // Log every 10 steps
        if step % 10 == 0 || step == steps - 1 {
            pgrx::log!(
                "Step {}/{}: loss = {:.4}",
                step + 1,
                steps,
                loss
            );
        }
    }

    let final_loss = *losses.last().unwrap_or(&0.0);
    let duration_ms = start.elapsed().as_millis() as i32;

    // Save weights back
    store_weights(&agent_id, &model).unwrap_or_else(|e| error!("{e}"));

    // Log training run
    let config_json = serde_json::json!({
        "dim": config.dim,
        "n_heads": config.n_heads,
        "n_layers": config.n_layers,
        "context_len": config.context_len,
        "lr": learning_rate,
        "batch_size": batch_size,
    });
    let scope_sql = match scope {
        Some(s) => format!("'{}'", s.replace('\'', "''")),
        None => "NULL".to_string(),
    };
    let log_sql = format!(
        "INSERT INTO kerai.training_runs (agent_id, config, walk_type, scope, n_sequences, n_steps, final_loss, duration_ms)
         VALUES ('{agent_id}'::uuid, '{config_json}'::jsonb, '{walk}', {scope_sql}::ltree, {n_seq}, {steps}, {final_loss}, {duration_ms})"
    );
    Spi::run(&log_sql).unwrap_or_else(|e| error!("Failed to log training run: {e}"));

    // Mint training reward
    mint_training_reward(&agent_id, steps);

    pgrx::JsonB(serde_json::json!({
        "status": "trained",
        "agent": agent_name,
        "walk_type": walk,
        "n_sequences": n_seq,
        "n_steps": steps,
        "initial_loss": losses.first().unwrap_or(&0.0),
        "final_loss": final_loss,
        "duration_ms": duration_ms,
    }))
}

/// Predict next nodes given a context sequence.
#[pg_extern]
fn predict_next(
    agent_name: &str,
    context: pgrx::JsonB,
    top_k: default!(Option<i32>, "NULL"),
) -> pgrx::JsonB {
    let agent_id = agent_id_by_name(agent_name).unwrap_or_else(|e| error!("{e}"));
    let config = load_model_config(&agent_id).unwrap_or_else(|e| error!("{e}"));
    let model = load_weights(&agent_id, &config).unwrap_or_else(|e| error!("{e}"));
    let k = top_k.unwrap_or(10) as usize;

    // Parse context node UUIDs from JSON array
    let context_uuids: Vec<String> = match context.0.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        None => error!("context must be a JSON array of node UUID strings"),
    };

    // Map UUIDs to token indices
    let token_indices = walks::uuids_to_indices(&agent_id, &context_uuids)
        .unwrap_or_else(|e| error!("{e}"));

    if token_indices.is_empty() {
        error!("No context nodes found in model vocabulary");
    }

    // Run prediction
    let predictions = model.predict_next(&token_indices, k);

    // Map indices back to UUIDs
    let results = walks::indices_to_uuids(&agent_id, &predictions)
        .unwrap_or_else(|e| error!("{e}"));

    // Deduct inference cost
    deduct_inference_cost(&agent_id);

    // Log inference
    let context_array = context_uuids
        .iter()
        .map(|u| format!("'{}'::uuid", u))
        .collect::<Vec<_>>()
        .join(",");
    if let Some(first) = results.first() {
        let log_sql = format!(
            "INSERT INTO kerai.inference_log (agent_id, context_nodes, predicted, score)
             VALUES ('{agent_id}'::uuid, ARRAY[{context_array}], '{}'::uuid, {})",
            first.0, first.1
        );
        let _ = Spi::run(&log_sql);
    }

    pgrx::JsonB(serde_json::json!({
        "predictions": results.iter().map(|(uuid, prob)| {
            serde_json::json!({"node_id": uuid, "probability": prob})
        }).collect::<Vec<_>>(),
    }))
}

/// FTS candidates re-ranked by neural model.
#[pg_extern]
fn neural_search(
    agent_name: &str,
    query_text: &str,
    context_nodes: default!(Option<pgrx::JsonB>, "NULL"),
    limit: default!(Option<i32>, "NULL"),
) -> pgrx::JsonB {
    let agent_id = agent_id_by_name(agent_name).unwrap_or_else(|e| error!("{e}"));
    let config = load_model_config(&agent_id).unwrap_or_else(|e| error!("{e}"));
    let model = load_weights(&agent_id, &config).unwrap_or_else(|e| error!("{e}"));
    let lim = limit.unwrap_or(20) as usize;

    // Get FTS candidates
    let escaped_query = query_text.replace('\'', "''");
    let fts_sql = format!(
        "SELECT id::text, ts_rank(to_tsvector('english', COALESCE(content, '')),
                                  plainto_tsquery('english', '{}')) AS rank,
                kind, path::text
         FROM kerai.nodes
         WHERE to_tsvector('english', COALESCE(content, ''))
               @@ plainto_tsquery('english', '{}')
         ORDER BY rank DESC
         LIMIT {}",
        escaped_query, escaped_query, lim * 2
    );

    let mut candidates: Vec<(String, f64, String, String)> = Vec::new();
    Spi::connect(|client| {
        let tup_table = client
            .select(&fts_sql, None, &[])
            .unwrap_or_else(|e| error!("FTS query failed: {e}"));
        for row in tup_table {
            let id: String = row.get_by_name::<String, _>("id").ok().flatten().unwrap_or_default();
            let rank: f64 = row
                .get_by_name::<f32, _>("rank")
                .ok()
                .flatten()
                .unwrap_or(0.0) as f64;
            let kind: String = row.get_by_name::<String, _>("kind").ok().flatten().unwrap_or_default();
            let path: String = row.get_by_name::<String, _>("path").ok().flatten().unwrap_or_default();
            candidates.push((id, rank, kind, path));
        }
    });

    if candidates.is_empty() {
        return pgrx::JsonB(serde_json::json!({"results": []}));
    }

    // Build context token indices
    let ctx_tokens = if let Some(ctx) = context_nodes {
        let uuids: Vec<String> = ctx
            .0
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        walks::uuids_to_indices(&agent_id, &uuids).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Score each candidate with the model
    let candidate_uuids: Vec<String> = candidates.iter().map(|(id, _, _, _)| id.clone()).collect();
    let candidate_indices = walks::uuids_to_indices(&agent_id, &candidate_uuids).unwrap_or_default();

    // If we have context, run forward pass and get probabilities for candidates
    let neural_scores: Vec<f64> = if !ctx_tokens.is_empty() && !candidate_indices.is_empty() {
        let (logits, _) = model.forward(&ctx_tokens);
        let vocab = config.vocab_size;
        let seq_len = ctx_tokens.len().min(config.context_len);
        let last_start = (seq_len - 1) * vocab;
        let last_logits = &logits.data[last_start..last_start + vocab];

        // Softmax
        let max_val = last_logits
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = last_logits.iter().map(|&v| (v - max_val).exp()).collect();
        let sum: f32 = exps.iter().sum();

        candidate_indices
            .iter()
            .map(|&idx| {
                if idx < vocab {
                    (exps[idx] / sum) as f64
                } else {
                    0.0
                }
            })
            .collect()
    } else {
        vec![0.0; candidates.len()]
    };

    // Combine FTS rank and neural score
    let mut results: Vec<serde_json::Value> = candidates
        .iter()
        .zip(neural_scores.iter())
        .map(|((id, fts_rank, kind, path), neural_score)| {
            let combined = fts_rank * (1.0 + neural_score);
            serde_json::json!({
                "node_id": id,
                "fts_rank": fts_rank,
                "neural_score": neural_score,
                "combined_score": combined,
                "kind": kind,
                "path": path,
            })
        })
        .collect();

    // Sort by combined score descending
    results.sort_by(|a, b| {
        let sa = a["combined_score"].as_f64().unwrap_or(0.0);
        let sb = b["combined_score"].as_f64().unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(lim);

    // Deduct inference cost
    deduct_inference_cost(&agent_id);

    pgrx::JsonB(serde_json::json!({"results": results}))
}

/// Average logits from multiple models.
#[pg_extern]
fn ensemble_predict(
    agent_names: pgrx::JsonB,
    context: pgrx::JsonB,
    top_k: default!(Option<i32>, "NULL"),
) -> pgrx::JsonB {
    let names: Vec<String> = match agent_names.0.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        None => error!("agent_names must be a JSON array of strings"),
    };

    if names.is_empty() {
        error!("At least one agent name required");
    }

    let context_uuids: Vec<String> = match context.0.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        None => error!("context must be a JSON array of node UUID strings"),
    };

    let k = top_k.unwrap_or(10) as usize;

    // Find the max vocab size across models for averaging
    let mut all_logits: Vec<Vec<f32>> = Vec::new();
    let mut max_vocab = 0usize;
    let mut agent_ids = Vec::new();

    for name in &names {
        let aid = agent_id_by_name(name).unwrap_or_else(|e| error!("{e}"));
        let cfg = load_model_config(&aid).unwrap_or_else(|e| error!("{e}"));
        let mdl = load_weights(&aid, &cfg).unwrap_or_else(|e| error!("{e}"));

        let indices = walks::uuids_to_indices(&aid, &context_uuids).unwrap_or_default();
        if indices.is_empty() {
            continue;
        }

        let (logits, _) = mdl.forward(&indices);
        let seq_len = indices.len().min(cfg.context_len);
        let last_start = (seq_len - 1) * cfg.vocab_size;
        let last_logits = logits.data[last_start..last_start + cfg.vocab_size].to_vec();

        if cfg.vocab_size > max_vocab {
            max_vocab = cfg.vocab_size;
        }
        all_logits.push(last_logits);
        agent_ids.push(aid);
    }

    if all_logits.is_empty() {
        error!("No models produced logits");
    }

    // Average logits (pad shorter ones with 0)
    let mut avg = vec![0.0f32; max_vocab];
    let n = all_logits.len() as f32;
    for logits in &all_logits {
        for (i, &v) in logits.iter().enumerate() {
            avg[i] += v / n;
        }
    }

    // Softmax
    let max_val = avg.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = avg.iter().map(|&v| (v - max_val).exp()).collect();
    let sum: f32 = exps.iter().sum();
    let probs: Vec<f32> = exps.iter().map(|&e| e / sum).collect();

    // Top-k — use the first agent's vocab for index→UUID mapping
    let mut indexed: Vec<(usize, f32)> = probs.into_iter().enumerate().collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(k);

    let results = walks::indices_to_uuids(&agent_ids[0], &indexed)
        .unwrap_or_else(|e| error!("{e}"));

    pgrx::JsonB(serde_json::json!({
        "models": names,
        "predictions": results.iter().map(|(uuid, prob)| {
            serde_json::json!({"node_id": uuid, "probability": prob})
        }).collect::<Vec<_>>(),
    }))
}

/// Model info: architecture, param count, training history.
#[pg_extern]
fn model_info(agent_name: &str) -> pgrx::JsonB {
    let agent_id = agent_id_by_name(agent_name).unwrap_or_else(|e| error!("{e}"));
    let config = load_model_config(&agent_id).unwrap_or_else(|e| error!("{e}"));

    // Count vocab
    let vocab_count_sql = format!(
        "SELECT count(*)::int FROM kerai.model_vocab WHERE model_id = '{agent_id}'::uuid"
    );
    let vocab_count: i32 = Spi::get_one(&vocab_count_sql)
        .ok()
        .flatten()
        .unwrap_or(0);

    // Count weight tensors
    let weight_count_sql = format!(
        "SELECT count(*)::int FROM kerai.model_weights WHERE agent_id = '{agent_id}'::uuid"
    );
    let weight_count: i32 = Spi::get_one(&weight_count_sql)
        .ok()
        .flatten()
        .unwrap_or(0);

    // Total weight bytes
    let bytes_sql = format!(
        "SELECT COALESCE(sum(octet_length(tensor_data)), 0)::bigint FROM kerai.model_weights WHERE agent_id = '{agent_id}'::uuid"
    );
    let total_bytes: i64 = Spi::get_one(&bytes_sql)
        .ok()
        .flatten()
        .unwrap_or(0);

    // Training history
    let history_sql = format!(
        "SELECT walk_type, n_steps, final_loss, duration_ms, created_at::text
         FROM kerai.training_runs
         WHERE agent_id = '{agent_id}'::uuid
         ORDER BY created_at DESC LIMIT 10"
    );
    let mut runs = Vec::new();
    Spi::connect(|client| {
        if let Ok(tup_table) = client.select(&history_sql, None, &[]) {
            for row in tup_table {
                let walk: String = row.get_by_name::<String, _>("walk_type").ok().flatten().unwrap_or_default();
                let steps: i32 = row.get_by_name::<i32, _>("n_steps").ok().flatten().unwrap_or(0);
                let loss: f64 = row.get_by_name::<f64, _>("final_loss").ok().flatten().unwrap_or(0.0);
                let dur: i32 = row.get_by_name::<i32, _>("duration_ms").ok().flatten().unwrap_or(0);
                let ts: String = row.get_by_name::<String, _>("created_at").ok().flatten().unwrap_or_default();
                runs.push(serde_json::json!({
                    "walk_type": walk, "n_steps": steps,
                    "final_loss": loss, "duration_ms": dur, "created_at": ts,
                }));
            }
        }
    });

    // Compute param count
    let param_count = {
        let dim = config.dim;
        let n_layers = config.n_layers;
        let per_layer = 4 * dim * dim + dim * 4 * dim + 4 * dim * dim + 2 * dim;
        config.vocab_size * dim + config.context_len * dim + n_layers * per_layer + dim
    };

    pgrx::JsonB(serde_json::json!({
        "agent": agent_name,
        "vocab_size": config.vocab_size,
        "dim": config.dim,
        "n_heads": config.n_heads,
        "n_layers": config.n_layers,
        "context_len": config.context_len,
        "param_count": param_count,
        "weight_bytes": total_bytes,
        "weight_tensors": weight_count,
        "vocab_entries": vocab_count,
        "training_runs": runs,
    }))
}

/// Delete a model's weights and vocabulary.
#[pg_extern]
fn delete_model(agent_name: &str) -> pgrx::JsonB {
    let agent_id = agent_id_by_name(agent_name).unwrap_or_else(|e| error!("{e}"));

    let del_weights = format!(
        "DELETE FROM kerai.model_weights WHERE agent_id = '{agent_id}'::uuid"
    );
    let del_vocab = format!(
        "DELETE FROM kerai.model_vocab WHERE model_id = '{agent_id}'::uuid"
    );
    let del_runs = format!(
        "DELETE FROM kerai.training_runs WHERE agent_id = '{agent_id}'::uuid"
    );
    let del_log = format!(
        "DELETE FROM kerai.inference_log WHERE agent_id = '{agent_id}'::uuid"
    );

    Spi::run(&del_weights).unwrap_or_else(|e| error!("Failed to delete weights: {e}"));
    Spi::run(&del_vocab).unwrap_or_else(|e| error!("Failed to delete vocab: {e}"));
    Spi::run(&del_runs).unwrap_or_else(|e| error!("Failed to delete runs: {e}"));
    Spi::run(&del_log).unwrap_or_else(|e| error!("Failed to delete log: {e}"));

    // Clear model config
    let clear_config = format!(
        "UPDATE kerai.agents SET config = '{{}}'::jsonb WHERE id = '{agent_id}'::uuid"
    );
    Spi::run(&clear_config).unwrap_or_else(|e| error!("Failed to clear config: {e}"));

    pgrx::JsonB(serde_json::json!({
        "status": "deleted",
        "agent": agent_name,
    }))
}

/// Mark an inference log entry as selected (for feedback learning).
#[pg_extern]
fn record_selection(inference_id: pgrx::Uuid) -> pgrx::JsonB {
    let id_str = uuid_to_string(inference_id);
    let sql = format!(
        "UPDATE kerai.inference_log SET selected = true WHERE id = '{id_str}'::uuid RETURNING id::text"
    );
    let updated = Spi::get_one::<String>(&sql)
        .ok()
        .flatten();

    match updated {
        Some(id) => pgrx::JsonB(serde_json::json!({"status": "recorded", "id": id})),
        None => error!("Inference log entry not found: {}", id_str),
    }
}

fn uuid_to_string(u: pgrx::Uuid) -> String {
    let bytes = u.as_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

/// Mint Koi reward for training.
fn mint_training_reward(agent_id: &str, steps: usize) {
    // Look up the agent's wallet
    let wallet_sql = format!(
        "SELECT w.id::text FROM kerai.wallets w
         JOIN kerai.agents a ON a.wallet_id = w.id
         WHERE a.id = '{agent_id}'::uuid"
    );
    let wallet_id = match Spi::get_one::<String>(&wallet_sql) {
        Ok(Some(w)) => w,
        _ => return, // No wallet, skip reward
    };

    // Look up reward schedule
    let reward_sql =
        "SELECT reward FROM kerai.reward_schedule WHERE work_type = 'model_training' AND enabled = true";
    let base_reward = match Spi::get_one::<i64>(reward_sql) {
        Ok(Some(r)) => r,
        _ => return, // No reward configured
    };

    let reward = base_reward * (steps as i64 / 10).max(1);

    // Mint
    let ts_sql = "SELECT COALESCE(max(lamport_ts), 0) + 1 FROM kerai.operations";
    let ts: i64 = Spi::get_one(ts_sql).ok().flatten().unwrap_or(1);

    let mint_sql = format!(
        "INSERT INTO kerai.ledger (to_wallet, amount, reason, timestamp)
         VALUES ('{wallet_id}'::uuid, {reward}, 'model_training', {ts})"
    );
    let _ = Spi::run(&mint_sql);

    let log_sql = format!(
        "INSERT INTO kerai.reward_log (work_type, reward, wallet_id, details)
         VALUES ('model_training', {reward}, '{wallet_id}'::uuid,
                 '{{\"agent_id\": \"{agent_id}\", \"steps\": {steps}}}'::jsonb)"
    );
    let _ = Spi::run(&log_sql);
}

/// Deduct Koi for inference.
fn deduct_inference_cost(agent_id: &str) {
    // Look up the agent's wallet
    let wallet_sql = format!(
        "SELECT w.id::text FROM kerai.wallets w
         JOIN kerai.agents a ON a.wallet_id = w.id
         WHERE a.id = '{agent_id}'::uuid"
    );
    let wallet_id = match Spi::get_one::<String>(&wallet_sql) {
        Ok(Some(w)) => w,
        _ => return,
    };

    // Look up inference pricing
    let price_sql =
        "SELECT unit_cost FROM kerai.pricing WHERE resource_type = 'model_inference' LIMIT 1";
    let cost = match Spi::get_one::<i64>(price_sql) {
        Ok(Some(c)) => c,
        _ => return, // No pricing configured
    };

    // Check balance
    let bal_sql = format!(
        "SELECT COALESCE(
            (SELECT sum(amount) FROM kerai.ledger WHERE to_wallet = '{wallet_id}'::uuid), 0
        ) - COALESCE(
            (SELECT sum(amount) FROM kerai.ledger WHERE from_wallet = '{wallet_id}'::uuid), 0
        )"
    );
    let balance: i64 = Spi::get_one(&bal_sql).ok().flatten().unwrap_or(0);
    if balance < cost {
        return; // Insufficient funds, skip (don't block inference)
    }

    let ts_sql = "SELECT COALESCE(max(lamport_ts), 0) + 1 FROM kerai.operations";
    let ts: i64 = Spi::get_one(ts_sql).ok().flatten().unwrap_or(1);

    let deduct_sql = format!(
        "INSERT INTO kerai.ledger (from_wallet, to_wallet, amount, reason, timestamp)
         VALUES ('{wallet_id}'::uuid,
                 (SELECT id FROM kerai.wallets WHERE instance_id = (SELECT id FROM kerai.instances WHERE is_self = true) LIMIT 1),
                 {cost}, 'model_inference', {ts})"
    );
    let _ = Spi::run(&deduct_sql);
}
