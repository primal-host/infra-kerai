use rand::Rng;

/// A simple tensor backed by a flat Vec<f32>.
#[derive(Clone, Debug)]
pub struct Tensor {
    pub data: Vec<f32>,
    pub shape: Vec<usize>,
}

impl Tensor {
    /// Create a new tensor with the given shape filled with zeros.
    pub fn zeros(shape: &[usize]) -> Self {
        let n: usize = shape.iter().product();
        Self {
            data: vec![0.0; n],
            shape: shape.to_vec(),
        }
    }

    /// Create a tensor filled with ones.
    pub fn ones(shape: &[usize]) -> Self {
        let n: usize = shape.iter().product();
        Self {
            data: vec![1.0; n],
            shape: shape.to_vec(),
        }
    }

    /// Xavier-initialized random tensor: N(0, sqrt(2 / (fan_in + fan_out))).
    pub fn randn_xavier(shape: &[usize]) -> Self {
        let n: usize = shape.iter().product();
        let fan_in = if shape.len() >= 2 { shape[shape.len() - 1] } else { shape[0] };
        let fan_out = shape[0];
        let std = (2.0 / (fan_in + fan_out) as f64).sqrt() as f32;
        let mut rng = rand::thread_rng();
        let data: Vec<f32> = (0..n)
            .map(|_| {
                // Box-Muller transform for normal distribution
                let u1: f32 = rng.gen::<f32>().max(1e-7);
                let u2: f32 = rng.gen();
                let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
                z * std
            })
            .collect();
        Self {
            data,
            shape: shape.to_vec(),
        }
    }

    /// Total number of elements.
    pub fn numel(&self) -> usize {
        self.data.len()
    }

    /// 2D matrix multiply: [M, K] x [K, N] -> [M, N].
    pub fn matmul(&self, other: &Tensor) -> Tensor {
        assert!(self.shape.len() == 2 && other.shape.len() == 2);
        let m = self.shape[0];
        let k = self.shape[1];
        assert_eq!(k, other.shape[0]);
        let n = other.shape[1];
        let mut out = vec![0.0f32; m * n];
        for i in 0..m {
            for j in 0..n {
                let mut sum = 0.0f32;
                for p in 0..k {
                    sum += self.data[i * k + p] * other.data[p * n + j];
                }
                out[i * n + j] = sum;
            }
        }
        Tensor {
            data: out,
            shape: vec![m, n],
        }
    }

    /// Batched matmul: [B, M, K] x [K, N] -> [B, M, N].
    /// The right-hand side is a 2D matrix broadcast across batches.
    pub fn batched_matmul(&self, weight: &Tensor) -> Tensor {
        assert_eq!(self.shape.len(), 3);
        assert_eq!(weight.shape.len(), 2);
        let b = self.shape[0];
        let m = self.shape[1];
        let k = self.shape[2];
        assert_eq!(k, weight.shape[0]);
        let n = weight.shape[1];
        let mut out = vec![0.0f32; b * m * n];
        for batch in 0..b {
            for i in 0..m {
                for j in 0..n {
                    let mut sum = 0.0f32;
                    for p in 0..k {
                        sum += self.data[batch * m * k + i * k + p] * weight.data[p * n + j];
                    }
                    out[batch * m * n + i * n + j] = sum;
                }
            }
        }
        Tensor {
            data: out,
            shape: vec![b, m, n],
        }
    }

    /// Element-wise addition (shapes must match or broadcast last dims).
    pub fn add(&self, other: &Tensor) -> Tensor {
        if self.data.len() == other.data.len() {
            let data: Vec<f32> = self
                .data
                .iter()
                .zip(other.data.iter())
                .map(|(a, b)| a + b)
                .collect();
            Tensor {
                data,
                shape: self.shape.clone(),
            }
        } else {
            // Broadcast: other is smaller and repeats across leading dims
            let other_n = other.data.len();
            assert_eq!(self.data.len() % other_n, 0);
            let data: Vec<f32> = self
                .data
                .iter()
                .enumerate()
                .map(|(i, &v)| v + other.data[i % other_n])
                .collect();
            Tensor {
                data,
                shape: self.shape.clone(),
            }
        }
    }

    /// Element-wise addition in place.
    pub fn add_inplace(&mut self, other: &Tensor) {
        if self.data.len() == other.data.len() {
            for (a, b) in self.data.iter_mut().zip(other.data.iter()) {
                *a += b;
            }
        } else {
            let other_n = other.data.len();
            assert_eq!(self.data.len() % other_n, 0);
            for (i, v) in self.data.iter_mut().enumerate() {
                *v += other.data[i % other_n];
            }
        }
    }

    /// Scalar multiplication.
    pub fn mul_scalar(&self, s: f32) -> Tensor {
        Tensor {
            data: self.data.iter().map(|&v| v * s).collect(),
            shape: self.shape.clone(),
        }
    }

    /// Element-wise multiply (Hadamard product).
    pub fn mul_elementwise(&self, other: &Tensor) -> Tensor {
        assert_eq!(self.data.len(), other.data.len());
        Tensor {
            data: self
                .data
                .iter()
                .zip(other.data.iter())
                .map(|(a, b)| a * b)
                .collect(),
            shape: self.shape.clone(),
        }
    }

    /// ReLU activation.
    pub fn relu(&self) -> Tensor {
        Tensor {
            data: self.data.iter().map(|&v| v.max(0.0)).collect(),
            shape: self.shape.clone(),
        }
    }

    /// ReLU derivative mask: 1.0 where data > 0, else 0.0.
    pub fn relu_mask(&self) -> Tensor {
        Tensor {
            data: self
                .data
                .iter()
                .map(|&v| if v > 0.0 { 1.0 } else { 0.0 })
                .collect(),
            shape: self.shape.clone(),
        }
    }

    /// Row-wise softmax for 2D tensor [rows, cols].
    pub fn softmax(&self) -> Tensor {
        assert_eq!(self.shape.len(), 2);
        let rows = self.shape[0];
        let cols = self.shape[1];
        let mut data = vec![0.0f32; rows * cols];
        for r in 0..rows {
            let start = r * cols;
            let row = &self.data[start..start + cols];
            let max_val = row.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let mut sum = 0.0f32;
            for c in 0..cols {
                let e = (row[c] - max_val).exp();
                data[start + c] = e;
                sum += e;
            }
            for c in 0..cols {
                data[start + c] /= sum;
            }
        }
        Tensor {
            data,
            shape: self.shape.clone(),
        }
    }

    /// RMSNorm: x / rms(x) * gamma, where rms = sqrt(mean(x^2) + eps).
    /// `gamma` has shape [dim], self has shape [..., dim].
    pub fn rms_norm(&self, gamma: &Tensor, eps: f32) -> Tensor {
        let dim = *self.shape.last().unwrap();
        assert_eq!(gamma.data.len(), dim);
        let n = self.data.len() / dim;
        let mut data = vec![0.0f32; self.data.len()];
        for i in 0..n {
            let start = i * dim;
            let slice = &self.data[start..start + dim];
            let rms = (slice.iter().map(|&x| x * x).sum::<f32>() / dim as f32 + eps).sqrt();
            for j in 0..dim {
                data[start + j] = slice[j] / rms * gamma.data[j];
            }
        }
        Tensor {
            data,
            shape: self.shape.clone(),
        }
    }

    /// Cross-entropy loss for next-token prediction.
    /// `logits` is [seq_len, vocab_size], `targets` is a slice of target indices.
    /// Returns average loss over the sequence.
    pub fn cross_entropy_loss(&self, targets: &[usize]) -> f32 {
        assert_eq!(self.shape.len(), 2);
        let seq_len = self.shape[0];
        let vocab_size = self.shape[1];
        assert_eq!(targets.len(), seq_len);
        let mut total_loss = 0.0f32;
        for t in 0..seq_len {
            let start = t * vocab_size;
            let row = &self.data[start..start + vocab_size];
            let max_val = row.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let sum_exp: f32 = row.iter().map(|&v| (v - max_val).exp()).sum();
            let log_sum_exp = max_val + sum_exp.ln();
            let target_logit = row[targets[t]];
            total_loss += log_sum_exp - target_logit;
        }
        total_loss / seq_len as f32
    }

    /// Transpose a 2D tensor.
    pub fn transpose(&self) -> Tensor {
        assert_eq!(self.shape.len(), 2);
        let rows = self.shape[0];
        let cols = self.shape[1];
        let mut data = vec![0.0f32; rows * cols];
        for r in 0..rows {
            for c in 0..cols {
                data[c * rows + r] = self.data[r * cols + c];
            }
        }
        Tensor {
            data,
            shape: vec![cols, rows],
        }
    }

    /// Extract a single row from a 2D tensor.
    pub fn slice_row(&self, row: usize) -> Tensor {
        assert_eq!(self.shape.len(), 2);
        let cols = self.shape[1];
        let start = row * cols;
        Tensor {
            data: self.data[start..start + cols].to_vec(),
            shape: vec![1, cols],
        }
    }

    /// Look up embeddings: given indices, return [len, dim] tensor from [vocab, dim] table.
    pub fn embed_lookup(&self, indices: &[usize]) -> Tensor {
        assert_eq!(self.shape.len(), 2);
        let dim = self.shape[1];
        let mut data = Vec::with_capacity(indices.len() * dim);
        for &idx in indices {
            let start = idx * dim;
            data.extend_from_slice(&self.data[start..start + dim]);
        }
        Tensor {
            data,
            shape: vec![indices.len(), dim],
        }
    }

    /// Serialize tensor data to little-endian bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.data.len() * 4);
        for &v in &self.data {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes
    }

    /// Deserialize from little-endian bytes.
    pub fn from_bytes(bytes: &[u8], shape: Vec<usize>) -> Self {
        assert_eq!(bytes.len() % 4, 0);
        let n = bytes.len() / 4;
        let expected: usize = shape.iter().product();
        assert_eq!(n, expected);
        let data: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Self { data, shape }
    }

    /// Reshape (total elements must match).
    pub fn reshape(&self, new_shape: Vec<usize>) -> Tensor {
        let n: usize = new_shape.iter().product();
        assert_eq!(n, self.data.len());
        Tensor {
            data: self.data.clone(),
            shape: new_shape,
        }
    }

    /// View as 2D: collapse all leading dims into rows.
    pub fn as_2d(&self) -> Tensor {
        let last = *self.shape.last().unwrap();
        let rows = self.data.len() / last;
        Tensor {
            data: self.data.clone(),
            shape: vec![rows, last],
        }
    }

    /// Sum along the last dimension: [..., N] -> [..., 1] squeezed.
    pub fn sum_last_dim(&self) -> Vec<f32> {
        let dim = *self.shape.last().unwrap();
        let n = self.data.len() / dim;
        let mut result = vec![0.0f32; n];
        for i in 0..n {
            let start = i * dim;
            result[i] = self.data[start..start + dim].iter().sum();
        }
        result
    }

    /// Create from a 2D Vec<Vec<f32>>.
    pub fn from_2d(rows: &[Vec<f32>]) -> Tensor {
        let r = rows.len();
        let c = if r > 0 { rows[0].len() } else { 0 };
        let mut data = Vec::with_capacity(r * c);
        for row in rows {
            assert_eq!(row.len(), c);
            data.extend_from_slice(row);
        }
        Tensor {
            data,
            shape: vec![r, c],
        }
    }

    /// Accumulate gradient: self += other.
    pub fn accumulate(&mut self, other: &Tensor) {
        assert_eq!(self.data.len(), other.data.len());
        for (a, b) in self.data.iter_mut().zip(other.data.iter()) {
            *a += b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matmul_identity() {
        let a = Tensor {
            data: vec![1.0, 2.0, 3.0, 4.0],
            shape: vec![2, 2],
        };
        let eye = Tensor {
            data: vec![1.0, 0.0, 0.0, 1.0],
            shape: vec![2, 2],
        };
        let result = a.matmul(&eye);
        assert_eq!(result.data, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_matmul_known() {
        // [[1,2],[3,4]] x [[5,6],[7,8]] = [[19,22],[43,50]]
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
    }

    #[test]
    fn test_softmax_sums_to_one() {
        let t = Tensor {
            data: vec![1.0, 2.0, 3.0, 4.0],
            shape: vec![1, 4],
        };
        let s = t.softmax();
        let sum: f32 = s.data.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_roundtrip_bytes() {
        let t = Tensor {
            data: vec![1.5, -2.3, 0.0, 42.0],
            shape: vec![2, 2],
        };
        let bytes = t.to_bytes();
        let t2 = Tensor::from_bytes(&bytes, vec![2, 2]);
        assert_eq!(t.data, t2.data);
    }

    #[test]
    fn test_transpose() {
        let t = Tensor {
            data: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            shape: vec![2, 3],
        };
        let tr = t.transpose();
        assert_eq!(tr.shape, vec![3, 2]);
        assert_eq!(tr.data, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn test_rms_norm() {
        let x = Tensor {
            data: vec![1.0, 2.0, 3.0, 4.0],
            shape: vec![2, 2],
        };
        let gamma = Tensor {
            data: vec![1.0, 1.0],
            shape: vec![2],
        };
        let normed = x.rms_norm(&gamma, 1e-5);
        // rms([1,2]) = sqrt((1+4)/2) = sqrt(2.5) ≈ 1.5811
        let rms = (2.5f32 + 1e-5).sqrt();
        assert!((normed.data[0] - 1.0 / rms).abs() < 1e-4);
        assert!((normed.data[1] - 2.0 / rms).abs() < 1e-4);
    }
}
