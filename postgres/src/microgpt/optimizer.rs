/// Adam optimizer with bias correction.
pub struct Adam {
    pub lr: f32,
    pub beta1: f32,
    pub beta2: f32,
    pub eps: f32,
    pub step: usize,
    /// First moment (mean of gradients).
    pub m: Vec<f32>,
    /// Second moment (mean of squared gradients).
    pub v: Vec<f32>,
}

impl Adam {
    pub fn new(n_params: usize, lr: f32) -> Self {
        Self {
            lr,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            step: 0,
            m: vec![0.0; n_params],
            v: vec![0.0; n_params],
        }
    }

    /// Perform one Adam update step. `params` and `grads` must have the same length.
    pub fn update(&mut self, params: &mut [f32], grads: &[f32]) {
        assert_eq!(params.len(), grads.len());
        assert_eq!(params.len(), self.m.len());
        self.step += 1;
        let bc1 = 1.0 - self.beta1.powi(self.step as i32);
        let bc2 = 1.0 - self.beta2.powi(self.step as i32);
        for i in 0..params.len() {
            self.m[i] = self.beta1 * self.m[i] + (1.0 - self.beta1) * grads[i];
            self.v[i] = self.beta2 * self.v[i] + (1.0 - self.beta2) * grads[i] * grads[i];
            let m_hat = self.m[i] / bc1;
            let v_hat = self.v[i] / bc2;
            params[i] -= self.lr * m_hat / (v_hat.sqrt() + self.eps);
        }
    }
}
