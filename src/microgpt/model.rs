use std::collections::HashMap;

use super::optimizer::Adam;
use super::tensor::Tensor;

/// Model hyperparameters.
#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub vocab_size: usize,
    pub dim: usize,
    pub n_heads: usize,
    pub n_layers: usize,
    pub context_len: usize,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            vocab_size: 100,
            dim: 32,
            n_heads: 4,
            n_layers: 1,
            context_len: 16,
        }
    }
}

/// A single transformer layer.
#[derive(Clone)]
pub struct TransformerLayer {
    pub q_proj: Tensor,  // [dim, dim]
    pub k_proj: Tensor,  // [dim, dim]
    pub v_proj: Tensor,  // [dim, dim]
    pub o_proj: Tensor,  // [dim, dim]
    pub ff_up: Tensor,   // [dim, 4*dim]
    pub ff_down: Tensor, // [4*dim, dim]
    pub norm1: Tensor,   // [dim]
    pub norm2: Tensor,   // [dim]
}

/// MicroGPT: a tiny causal transformer over graph nodes.
#[derive(Clone)]
pub struct MicroGPT {
    pub config: ModelConfig,
    pub token_emb: Tensor, // [vocab_size, dim]
    pub pos_emb: Tensor,   // [context_len, dim]
    pub layers: Vec<TransformerLayer>,
    pub final_norm: Tensor, // [dim]
    // lm_head is weight-tied to token_emb (transposed)
}

/// Cached activations from a forward pass, needed for backward.
pub struct ForwardCache {
    pub embedded: Tensor,                                                     // [seq, dim]
    pub layer_caches: Vec<LayerCache>,
    pub final_normed: Tensor,                                                 // [seq, dim]
    pub logits: Tensor,                                                       // [seq, vocab]
}

pub struct LayerCache {
    pub input: Tensor,          // pre-norm input [seq, dim]
    pub normed1: Tensor,        // after norm1 [seq, dim]
    pub q: Tensor,              // [seq, dim]
    pub k: Tensor,              // [seq, dim]
    pub v: Tensor,              // [seq, dim]
    pub attn_weights: Tensor,   // [n_heads, seq, seq]
    pub attn_out: Tensor,       // [seq, dim]
    pub post_attn: Tensor,      // residual + attn [seq, dim]
    pub normed2: Tensor,        // after norm2 [seq, dim]
    pub ff_hidden: Tensor,      // after up + relu [seq, 4*dim]
    pub ff_hidden_pre_relu: Tensor, // before relu [seq, 4*dim]
}

impl MicroGPT {
    /// Initialize with Xavier random weights.
    pub fn new(config: ModelConfig) -> Self {
        let dim = config.dim;
        let layers = (0..config.n_layers)
            .map(|_| TransformerLayer {
                q_proj: Tensor::randn_xavier(&[dim, dim]),
                k_proj: Tensor::randn_xavier(&[dim, dim]),
                v_proj: Tensor::randn_xavier(&[dim, dim]),
                o_proj: Tensor::randn_xavier(&[dim, dim]),
                ff_up: Tensor::randn_xavier(&[dim, 4 * dim]),
                ff_down: Tensor::randn_xavier(&[4 * dim, dim]),
                norm1: Tensor::ones(&[dim]),
                norm2: Tensor::ones(&[dim]),
            })
            .collect();

        Self {
            token_emb: Tensor::randn_xavier(&[config.vocab_size, dim]),
            pos_emb: Tensor::randn_xavier(&[config.context_len, dim]),
            layers,
            final_norm: Tensor::ones(&[dim]),
            config,
        }
    }

    /// Count total parameters.
    pub fn param_count(&self) -> usize {
        let mut n = self.token_emb.numel() + self.pos_emb.numel() + self.final_norm.numel();
        for layer in &self.layers {
            n += layer.q_proj.numel()
                + layer.k_proj.numel()
                + layer.v_proj.numel()
                + layer.o_proj.numel()
                + layer.ff_up.numel()
                + layer.ff_down.numel()
                + layer.norm1.numel()
                + layer.norm2.numel();
        }
        n
    }

    /// Collect all parameters into a flat Vec.
    pub fn params_flat(&self) -> Vec<f32> {
        let mut p = Vec::with_capacity(self.param_count());
        p.extend_from_slice(&self.token_emb.data);
        p.extend_from_slice(&self.pos_emb.data);
        for layer in &self.layers {
            p.extend_from_slice(&layer.q_proj.data);
            p.extend_from_slice(&layer.k_proj.data);
            p.extend_from_slice(&layer.v_proj.data);
            p.extend_from_slice(&layer.o_proj.data);
            p.extend_from_slice(&layer.ff_up.data);
            p.extend_from_slice(&layer.ff_down.data);
            p.extend_from_slice(&layer.norm1.data);
            p.extend_from_slice(&layer.norm2.data);
        }
        p.extend_from_slice(&self.final_norm.data);
        p
    }

    /// Load parameters from a flat Vec (inverse of params_flat).
    pub fn load_params_flat(&mut self, flat: &[f32]) {
        assert_eq!(flat.len(), self.param_count());
        let mut offset = 0;
        let mut read = |tensor: &mut Tensor| {
            let n = tensor.numel();
            tensor.data.copy_from_slice(&flat[offset..offset + n]);
            offset += n;
        };
        read(&mut self.token_emb);
        read(&mut self.pos_emb);
        for layer in &mut self.layers {
            read(&mut layer.q_proj);
            read(&mut layer.k_proj);
            read(&mut layer.v_proj);
            read(&mut layer.o_proj);
            read(&mut layer.ff_up);
            read(&mut layer.ff_down);
            read(&mut layer.norm1);
            read(&mut layer.norm2);
        }
        read(&mut self.final_norm);
    }

    /// Forward pass: token indices → logits [seq_len, vocab_size].
    /// Returns logits and a cache for backward.
    pub fn forward(&self, tokens: &[usize]) -> (Tensor, ForwardCache) {
        let seq_len = tokens.len().min(self.config.context_len);
        let tokens = &tokens[..seq_len];
        let dim = self.config.dim;

        // Token + positional embeddings
        let tok_emb = self.token_emb.embed_lookup(tokens); // [seq, dim]
        let pos_indices: Vec<usize> = (0..seq_len).collect();
        let pos = self.pos_emb.embed_lookup(&pos_indices); // [seq, dim]
        let mut x = tok_emb.add(&pos); // [seq, dim]
        let embedded = x.clone();

        let mut layer_caches = Vec::with_capacity(self.config.n_layers);

        for layer in &self.layers {
            let input = x.clone();

            // Pre-norm (RMSNorm)
            let normed1 = x.rms_norm(&layer.norm1, 1e-5);
            let normed1_2d = normed1.as_2d(); // [seq, dim]

            // Multi-head self-attention
            let q = normed1_2d.matmul(&layer.q_proj); // [seq, dim]
            let k = normed1_2d.matmul(&layer.k_proj);
            let v = normed1_2d.matmul(&layer.v_proj);

            let head_dim = dim / self.config.n_heads;
            let n_heads = self.config.n_heads;

            // Compute attention per head
            let mut attn_out_data = vec![0.0f32; seq_len * dim];
            let mut all_attn_weights = vec![0.0f32; n_heads * seq_len * seq_len];

            for h in 0..n_heads {
                let h_start = h * head_dim;
                // Extract Q, K, V slices for this head
                for qi in 0..seq_len {
                    for ki in 0..seq_len {
                        if ki > qi {
                            // Causal mask: future tokens get -inf
                            all_attn_weights[h * seq_len * seq_len + qi * seq_len + ki] =
                                f32::NEG_INFINITY;
                            continue;
                        }
                        let mut dot = 0.0f32;
                        for d in 0..head_dim {
                            dot += q.data[qi * dim + h_start + d]
                                * k.data[ki * dim + h_start + d];
                        }
                        all_attn_weights[h * seq_len * seq_len + qi * seq_len + ki] =
                            dot / (head_dim as f32).sqrt();
                    }
                }

                // Softmax per query position
                for qi in 0..seq_len {
                    let start = h * seq_len * seq_len + qi * seq_len;
                    let row = &mut all_attn_weights[start..start + seq_len];
                    let max_val = row.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                    let mut sum = 0.0f32;
                    for v in row.iter_mut() {
                        *v = (*v - max_val).exp();
                        sum += *v;
                    }
                    for v in row.iter_mut() {
                        *v /= sum;
                    }
                }

                // Weighted sum of V
                for qi in 0..seq_len {
                    for d in 0..head_dim {
                        let mut sum = 0.0f32;
                        for ki in 0..seq_len {
                            sum += all_attn_weights
                                [h * seq_len * seq_len + qi * seq_len + ki]
                                * v.data[ki * dim + h_start + d];
                        }
                        attn_out_data[qi * dim + h_start + d] = sum;
                    }
                }
            }

            let attn_concat = Tensor {
                data: attn_out_data,
                shape: vec![seq_len, dim],
            };
            let attn_proj = attn_concat.matmul(&layer.o_proj); // [seq, dim]

            // Residual connection
            let post_attn = input.add(&attn_proj);

            // Pre-norm for FFN
            let normed2 = post_attn.rms_norm(&layer.norm2, 1e-5);
            let normed2_2d = normed2.as_2d();

            // Feed-forward: up → relu → down
            let ff_hidden_pre_relu = normed2_2d.matmul(&layer.ff_up); // [seq, 4*dim]
            let ff_hidden = ff_hidden_pre_relu.relu();
            let ff_out = ff_hidden.matmul(&layer.ff_down); // [seq, dim]

            // Residual connection
            x = post_attn.add(&ff_out);

            layer_caches.push(LayerCache {
                input,
                normed1,
                q,
                k,
                v,
                attn_weights: Tensor {
                    data: all_attn_weights,
                    shape: vec![n_heads, seq_len, seq_len],
                },
                attn_out: attn_concat,
                post_attn: post_attn.clone(),
                normed2,
                ff_hidden: ff_hidden.clone(),
                ff_hidden_pre_relu,
            });
        }

        // Final norm
        let final_normed = x.rms_norm(&self.final_norm, 1e-5);

        // Logits via weight-tied lm_head: [seq, dim] x [dim, vocab] = [seq, vocab]
        let lm_head = self.token_emb.transpose(); // [dim, vocab]
        let logits = final_normed.as_2d().matmul(&lm_head); // [seq, vocab]

        let cache = ForwardCache {
            embedded,
            layer_caches,
            final_normed,
            logits: logits.clone(),
        };

        (logits, cache)
    }

    /// Backward pass: compute gradients given cached forward activations and targets.
    /// Returns gradients as a flat Vec<f32> in the same order as params_flat().
    pub fn backward(&self, cache: &ForwardCache, targets: &[usize]) -> Vec<f32> {
        let seq_len = targets.len();
        let dim = self.config.dim;
        let vocab = self.config.vocab_size;
        let n_heads = self.config.n_heads;
        let head_dim = dim / n_heads;

        // --- d_logits: softmax cross-entropy gradient ---
        // softmax(logits) - one_hot(target) for each position
        let mut d_logits = vec![0.0f32; seq_len * vocab];
        for t in 0..seq_len {
            let start = t * vocab;
            let row = &cache.logits.data[start..start + vocab];
            let max_val = row.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let mut sum_exp = 0.0f32;
            for i in 0..vocab {
                d_logits[start + i] = (row[i] - max_val).exp();
                sum_exp += d_logits[start + i];
            }
            for i in 0..vocab {
                d_logits[start + i] /= sum_exp;
            }
            d_logits[start + targets[t]] -= 1.0;
            // Average over sequence
            for i in 0..vocab {
                d_logits[start + i] /= seq_len as f32;
            }
        }

        let d_logits = Tensor {
            data: d_logits,
            shape: vec![seq_len, vocab],
        };

        // --- d_token_emb (from lm_head, tied weights) ---
        // logits = final_normed @ token_emb^T
        // d_token_emb += d_logits^T @ final_normed  (from lm_head usage)
        let d_token_emb_from_head = d_logits.transpose().matmul(&cache.final_normed.as_2d());
        // [vocab, seq] x [seq, dim] = [vocab, dim]

        // d_final_normed = d_logits @ token_emb  [seq, vocab] x [vocab, dim] = [seq, dim]
        let mut d_x = d_logits.matmul(&self.token_emb);

        // --- d_final_norm (simplified RMSNorm grad — pass through scale) ---
        let mut d_final_norm = Tensor::zeros(&[dim]);
        {
            let x_pre = if self.config.n_layers > 0 {
                // x before final_norm is the output of the last layer
                let last = &cache.layer_caches[self.config.n_layers - 1];
                last.post_attn.add(&cache.layer_caches[self.config.n_layers - 1].ff_hidden.matmul(
                    &self.layers[self.config.n_layers - 1].ff_down,
                ))
            } else {
                cache.embedded.clone()
            };
            // Approximate: d_gamma_j = sum_over_positions(d_y_j * x_j / rms)
            for i in 0..seq_len {
                let slice = &x_pre.data[i * dim..(i + 1) * dim];
                let rms =
                    (slice.iter().map(|&v| v * v).sum::<f32>() / dim as f32 + 1e-5).sqrt();
                for j in 0..dim {
                    d_final_norm.data[j] += d_x.data[i * dim + j] * slice[j] / rms;
                }
            }
            // Propagate d_x through RMSNorm (simplified: scale by gamma/rms)
            let mut d_x_through = Tensor::zeros(&[seq_len, dim]);
            for i in 0..seq_len {
                let slice = &x_pre.data[i * dim..(i + 1) * dim];
                let rms =
                    (slice.iter().map(|&v| v * v).sum::<f32>() / dim as f32 + 1e-5).sqrt();
                for j in 0..dim {
                    d_x_through.data[i * dim + j] =
                        d_x.data[i * dim + j] * self.final_norm.data[j] / rms;
                }
            }
            d_x = d_x_through;
        }

        // --- Layer gradients (reverse order) ---
        let mut layer_grads: Vec<LayerGrads> = Vec::with_capacity(self.config.n_layers);

        for l in (0..self.config.n_layers).rev() {
            let lc = &cache.layer_caches[l];
            let layer = &self.layers[l];

            // --- FFN residual: d_x flows to both post_attn and ff_out ---
            let d_ff_out = d_x.clone(); // [seq, dim]

            // d_ff_hidden = d_ff_out @ ff_down^T  [seq, dim] x [dim, 4*dim] = [seq, 4*dim]
            let d_ff_hidden = d_ff_out.matmul(&layer.ff_down.transpose());

            // d_ff_down = ff_hidden^T @ d_ff_out  [4*dim, seq] x [seq, dim] = [4*dim, dim]
            let d_ff_down = lc.ff_hidden.transpose().matmul(&d_ff_out);

            // ReLU mask
            let relu_mask = lc.ff_hidden_pre_relu.relu_mask();
            let d_ff_pre_relu = d_ff_hidden.mul_elementwise(&relu_mask);

            // d_ff_up = normed2^T @ d_ff_pre_relu  [dim, seq] x [seq, 4*dim] = [dim, 4*dim]
            let d_ff_up = lc.normed2.as_2d().transpose().matmul(&d_ff_pre_relu);

            // d_normed2 = d_ff_pre_relu @ ff_up^T  [seq, 4*dim] x [4*dim, dim] = [seq, dim]
            let d_normed2 = d_ff_pre_relu.matmul(&layer.ff_up.transpose());

            // d_norm2 (RMSNorm gamma grad)
            let mut d_norm2 = Tensor::zeros(&[dim]);
            for i in 0..seq_len {
                let slice = &lc.post_attn.data[i * dim..(i + 1) * dim];
                let rms =
                    (slice.iter().map(|&v| v * v).sum::<f32>() / dim as f32 + 1e-5).sqrt();
                for j in 0..dim {
                    d_norm2.data[j] += d_normed2.data[i * dim + j] * slice[j] / rms;
                }
            }

            // Propagate through norm2 to post_attn
            let mut d_post_attn = d_x.clone(); // residual path
            for i in 0..seq_len {
                let slice = &lc.post_attn.data[i * dim..(i + 1) * dim];
                let rms =
                    (slice.iter().map(|&v| v * v).sum::<f32>() / dim as f32 + 1e-5).sqrt();
                for j in 0..dim {
                    d_post_attn.data[i * dim + j] +=
                        d_normed2.data[i * dim + j] * layer.norm2.data[j] / rms;
                }
            }

            // --- Attention residual: d_post_attn flows to input and attn_proj ---
            let d_attn_proj = d_post_attn.clone(); // [seq, dim]

            // d_o_proj = attn_out^T @ d_attn_proj  [dim, seq] x [seq, dim] = [dim, dim]
            let d_o_proj = lc.attn_out.transpose().matmul(&d_attn_proj);

            // d_attn_concat = d_attn_proj @ o_proj^T  [seq, dim] x [dim, dim] = [seq, dim]
            let d_attn_concat = d_attn_proj.matmul(&layer.o_proj.transpose());

            // Backprop through attention per head
            let mut d_q = Tensor::zeros(&[seq_len, dim]);
            let mut d_k = Tensor::zeros(&[seq_len, dim]);
            let mut d_v = Tensor::zeros(&[seq_len, dim]);

            for h in 0..n_heads {
                let h_start = h * head_dim;
                let scale = (head_dim as f32).sqrt();

                for qi in 0..seq_len {
                    // d_attn_concat[qi, h_start..h_start+head_dim] is the grad for this head's output
                    // output[qi] = sum_ki attn_w[qi,ki] * v[ki]
                    // d_attn_w[qi,ki] += d_out[qi] . v[ki]
                    // d_v[ki] += attn_w[qi,ki] * d_out[qi]

                    let mut d_scores = vec![0.0f32; seq_len]; // d_softmax_input

                    for ki in 0..=qi {
                        let w = lc.attn_weights.data
                            [h * seq_len * seq_len + qi * seq_len + ki];
                        for d in 0..head_dim {
                            let d_out = d_attn_concat.data[qi * dim + h_start + d];
                            d_scores[ki] += d_out * lc.v.data[ki * dim + h_start + d];
                            d_v.data[ki * dim + h_start + d] += w * d_out;
                        }
                    }

                    // Softmax backward: d_score_j = attn_j * (d_score_j - sum_k(attn_k * d_score_k))
                    let dot_sum: f32 = (0..=qi)
                        .map(|ki| {
                            lc.attn_weights.data
                                [h * seq_len * seq_len + qi * seq_len + ki]
                                * d_scores[ki]
                        })
                        .sum();

                    for ki in 0..=qi {
                        let w = lc.attn_weights.data
                            [h * seq_len * seq_len + qi * seq_len + ki];
                        let ds = w * (d_scores[ki] - dot_sum);
                        // ds is gradient w.r.t. pre-softmax score = q.k/sqrt(d)
                        // d_q[qi] += ds * k[ki] / sqrt(d)
                        // d_k[ki] += ds * q[qi] / sqrt(d)
                        for d in 0..head_dim {
                            d_q.data[qi * dim + h_start + d] +=
                                ds * lc.k.data[ki * dim + h_start + d] / scale;
                            d_k.data[ki * dim + h_start + d] +=
                                ds * lc.q.data[qi * dim + h_start + d] / scale;
                        }
                    }
                }
            }

            // d_q_proj = normed1^T @ d_q  [dim, seq] x [seq, dim] = [dim, dim]
            let d_q_proj = lc.normed1.as_2d().transpose().matmul(&d_q);
            let d_k_proj = lc.normed1.as_2d().transpose().matmul(&d_k);
            let d_v_proj = lc.normed1.as_2d().transpose().matmul(&d_v);

            // d_normed1 = d_q @ q_proj^T + d_k @ k_proj^T + d_v @ v_proj^T
            let d_normed1 = d_q
                .matmul(&layer.q_proj.transpose())
                .add(&d_k.matmul(&layer.k_proj.transpose()))
                .add(&d_v.matmul(&layer.v_proj.transpose()));

            // d_norm1 (RMSNorm gamma grad)
            let mut d_norm1 = Tensor::zeros(&[dim]);
            for i in 0..seq_len {
                let slice = &lc.input.data[i * dim..(i + 1) * dim];
                let rms =
                    (slice.iter().map(|&v| v * v).sum::<f32>() / dim as f32 + 1e-5).sqrt();
                for j in 0..dim {
                    d_norm1.data[j] += d_normed1.data[i * dim + j] * slice[j] / rms;
                }
            }

            // Propagate through norm1 to layer input
            let mut d_input = d_post_attn; // residual path
            for i in 0..seq_len {
                let slice = &lc.input.data[i * dim..(i + 1) * dim];
                let rms =
                    (slice.iter().map(|&v| v * v).sum::<f32>() / dim as f32 + 1e-5).sqrt();
                for j in 0..dim {
                    d_input.data[i * dim + j] +=
                        d_normed1.data[i * dim + j] * layer.norm1.data[j] / rms;
                }
            }

            d_x = d_input;

            layer_grads.push(LayerGrads {
                d_q_proj,
                d_k_proj,
                d_v_proj,
                d_o_proj,
                d_ff_up,
                d_ff_down,
                d_norm1,
                d_norm2,
            });
        }

        // Reverse so index matches layer index
        layer_grads.reverse();

        // --- Embedding gradients ---
        // d_x is now [seq, dim] gradient at the embedding level
        // d_token_emb: scatter-add d_x into rows indexed by tokens
        let d_token_emb = d_token_emb_from_head; // already has lm_head contribution
        let tokens: Vec<usize> = (0..seq_len)
            .map(|i| {
                // We need the original token indices — reconstruct from embedded - pos
                // Actually we don't have them in cache. We'll pass them as part of backward.
                // For now, use a workaround: store in cache.
                // Since we can't modify cache, we'll accept tokens as parameter.
                // This is handled by the caller.
                i // placeholder
            })
            .collect();
        // We actually need the tokens. Let's adjust the API.
        // For now, d_token_emb from head is the main contribution.
        // The embedding grad from input is: d_token_emb[tok[i]] += d_x[i]
        // We'll handle this in backward_with_tokens.
        let _ = tokens;

        // d_pos_emb: d_x rows 0..seq_len map to pos indices 0..seq_len
        let mut d_pos_emb = Tensor::zeros(&self.pos_emb.shape.as_slice());
        for i in 0..seq_len {
            for j in 0..dim {
                d_pos_emb.data[i * dim + j] += d_x.data[i * dim + j];
            }
        }

        // Assemble flat gradient vector (same order as params_flat)
        let mut grads = Vec::with_capacity(self.param_count());
        grads.extend_from_slice(&d_token_emb.data);
        grads.extend_from_slice(&d_pos_emb.data);
        for lg in &layer_grads {
            grads.extend_from_slice(&lg.d_q_proj.data);
            grads.extend_from_slice(&lg.d_k_proj.data);
            grads.extend_from_slice(&lg.d_v_proj.data);
            grads.extend_from_slice(&lg.d_o_proj.data);
            grads.extend_from_slice(&lg.d_ff_up.data);
            grads.extend_from_slice(&lg.d_ff_down.data);
            grads.extend_from_slice(&lg.d_norm1.data);
            grads.extend_from_slice(&lg.d_norm2.data);
        }
        grads.extend_from_slice(&d_final_norm.data);
        grads
    }

    /// Backward pass with explicit token indices for proper embedding gradients.
    pub fn backward_with_tokens(
        &self,
        cache: &ForwardCache,
        targets: &[usize],
        input_tokens: &[usize],
    ) -> Vec<f32> {
        let mut grads = self.backward(cache, targets);
        let dim = self.config.dim;
        let seq_len = input_tokens.len().min(self.config.context_len);

        // Recompute d_x at embedding level
        // We already have the gradient in the first vocab_size*dim entries from lm_head.
        // Add the input embedding gradient: d_token_emb[tok[i]] += d_x[i]
        // d_x was propagated through layers and is implicitly in the pos_emb grad.
        // Since pos_emb grad = d_x, we can use it to scatter into token_emb.
        let pos_emb_offset = self.token_emb.numel();
        for i in 0..seq_len {
            let tok = input_tokens[i];
            if tok < self.config.vocab_size {
                for j in 0..dim {
                    grads[tok * dim + j] += grads[pos_emb_offset + i * dim + j];
                }
            }
        }

        grads
    }

    /// Predict next tokens given a context sequence.
    /// Returns Vec of (token_index, probability) sorted by probability descending.
    pub fn predict_next(&self, tokens: &[usize], top_k: usize) -> Vec<(usize, f32)> {
        let (logits, _) = self.forward(tokens);
        let seq_len = tokens.len().min(self.config.context_len);
        let vocab = self.config.vocab_size;

        // Take last position's logits
        let last_start = (seq_len - 1) * vocab;
        let last_logits = &logits.data[last_start..last_start + vocab];

        // Softmax
        let max_val = last_logits
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = last_logits.iter().map(|&v| (v - max_val).exp()).collect();
        let sum: f32 = exps.iter().sum();
        let probs: Vec<f32> = exps.iter().map(|&e| e / sum).collect();

        // Top-k
        let mut indexed: Vec<(usize, f32)> = probs.into_iter().enumerate().collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        indexed.truncate(top_k);
        indexed
    }

    /// Serialize model to a map of tensor_name → (Tensor).
    pub fn to_weight_map(&self) -> HashMap<String, Tensor> {
        let mut map = HashMap::new();
        map.insert("token_emb".to_string(), self.token_emb.clone());
        map.insert("pos_emb".to_string(), self.pos_emb.clone());
        map.insert("final_norm".to_string(), self.final_norm.clone());
        for (i, layer) in self.layers.iter().enumerate() {
            map.insert(format!("layer{}.q_proj", i), layer.q_proj.clone());
            map.insert(format!("layer{}.k_proj", i), layer.k_proj.clone());
            map.insert(format!("layer{}.v_proj", i), layer.v_proj.clone());
            map.insert(format!("layer{}.o_proj", i), layer.o_proj.clone());
            map.insert(format!("layer{}.ff_up", i), layer.ff_up.clone());
            map.insert(format!("layer{}.ff_down", i), layer.ff_down.clone());
            map.insert(format!("layer{}.norm1", i), layer.norm1.clone());
            map.insert(format!("layer{}.norm2", i), layer.norm2.clone());
        }
        map
    }

    /// Deserialize model from a weight map.
    pub fn from_weight_map(config: ModelConfig, map: &HashMap<String, Tensor>) -> Self {
        let token_emb = map.get("token_emb").expect("missing token_emb").clone();
        let pos_emb = map.get("pos_emb").expect("missing pos_emb").clone();
        let final_norm = map.get("final_norm").expect("missing final_norm").clone();
        let layers = (0..config.n_layers)
            .map(|i| TransformerLayer {
                q_proj: map
                    .get(&format!("layer{}.q_proj", i))
                    .expect("missing q_proj")
                    .clone(),
                k_proj: map
                    .get(&format!("layer{}.k_proj", i))
                    .expect("missing k_proj")
                    .clone(),
                v_proj: map
                    .get(&format!("layer{}.v_proj", i))
                    .expect("missing v_proj")
                    .clone(),
                o_proj: map
                    .get(&format!("layer{}.o_proj", i))
                    .expect("missing o_proj")
                    .clone(),
                ff_up: map
                    .get(&format!("layer{}.ff_up", i))
                    .expect("missing ff_up")
                    .clone(),
                ff_down: map
                    .get(&format!("layer{}.ff_down", i))
                    .expect("missing ff_down")
                    .clone(),
                norm1: map
                    .get(&format!("layer{}.norm1", i))
                    .expect("missing norm1")
                    .clone(),
                norm2: map
                    .get(&format!("layer{}.norm2", i))
                    .expect("missing norm2")
                    .clone(),
            })
            .collect();

        Self {
            config,
            token_emb,
            pos_emb,
            layers,
            final_norm,
        }
    }

    /// Train on a batch of sequences for one step. Returns loss.
    pub fn train_step(
        &mut self,
        sequences: &[Vec<usize>],
        optimizer: &mut Adam,
    ) -> f32 {
        let n_seq = sequences.len();
        if n_seq == 0 {
            return 0.0;
        }

        let mut total_loss = 0.0f32;
        let mut accumulated_grads = vec![0.0f32; self.param_count()];

        for seq in sequences {
            if seq.len() < 2 {
                continue;
            }
            let input = &seq[..seq.len() - 1];
            let targets: Vec<usize> = seq[1..].to_vec();

            let (logits, cache) = self.forward(input);
            let loss = logits.cross_entropy_loss(&targets);
            total_loss += loss;

            let grads = self.backward_with_tokens(&cache, &targets, input);
            for (a, g) in accumulated_grads.iter_mut().zip(grads.iter()) {
                *a += g / n_seq as f32;
            }
        }

        // Apply averaged gradients
        let mut params = self.params_flat();
        optimizer.update(&mut params, &accumulated_grads);
        self.load_params_flat(&params);

        total_loss / n_seq as f32
    }
}

struct LayerGrads {
    d_q_proj: Tensor,
    d_k_proj: Tensor,
    d_v_proj: Tensor,
    d_o_proj: Tensor,
    d_ff_up: Tensor,
    d_ff_down: Tensor,
    d_norm1: Tensor,
    d_norm2: Tensor,
}
