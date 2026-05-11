//! StableSwap invariant (Curve.fi) for like-priced assets.
//!
//! For two-coin pools with reserves `(x, y)` and amplification `A`, the
//! invariant `D` satisfies:
//!     A · n^n · S + D = A · D · n^n + D^(n+1) / (n^n · ∏ x_i)
//! where `n = 2`, `S = x + y`. We solve for `D` by Newton iteration and for
//! `y'` post-swap by another Newton iteration on the same equation.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StableSwapError {
    #[error("Newton iteration failed to converge")]
    DidNotConverge,
    #[error("amount must be positive")]
    NonPositiveInput,
}

#[derive(Debug, Clone, Copy)]
pub struct StableSwapPool {
    pub reserve_x: f64,
    pub reserve_y: f64,
    /// Amplification coefficient. Higher A ⇒ flatter curve near equilibrium.
    pub amplification: f64,
    pub fee: f64,
}

impl StableSwapPool {
    pub fn new(reserve_x: f64, reserve_y: f64, amplification: f64, fee: f64) -> Self {
        Self { reserve_x, reserve_y, amplification, fee }
    }

    /// Invariant `D` solved by Newton's method.
    pub fn invariant(&self) -> Result<f64, StableSwapError> {
        let n: f64 = 2.0;
        let s = self.reserve_x + self.reserve_y;
        let ann = self.amplification * n.powi(2);
        if s == 0.0 {
            return Ok(0.0);
        }
        let mut d = s;
        for _ in 0..64 {
            let d_p = d.powi(3) / (n.powi(2) * self.reserve_x * self.reserve_y);
            let d_prev = d;
            // Standard Curve update:
            //   D = (Ann · S + D_p · n) · D / ((Ann − 1) · D + (n + 1) · D_p)
            d = (ann * s + d_p * n) * d / ((ann - 1.0) * d + (n + 1.0) * d_p);
            if (d - d_prev).abs() < 1e-12 {
                return Ok(d);
            }
        }
        Err(StableSwapError::DidNotConverge)
    }

    /// Given X input, return Y output (gross of fee deduction on the way out).
    pub fn out_given_in_x_to_y(&self, dx: f64) -> Result<f64, StableSwapError> {
        if dx <= 0.0 {
            return Err(StableSwapError::NonPositiveInput);
        }
        let d = self.invariant()?;
        let x_new = self.reserve_x + dx;
        let n: f64 = 2.0;
        let ann = self.amplification * n.powi(2);
        // Solve   y^2 + (b − D) y − c = 0     with
        //   b = S' + D / Ann       where S' = x_new
        //   c = D^(n+1) / (n^n · x_new · Ann)
        let b = x_new + d / ann;
        let c = d.powi(3) / (n.powi(2) * x_new * ann);
        // Newton iteration for the quadratic-ish equation.
        let mut y = d;
        for _ in 0..64 {
            let y_prev = y;
            y = (y * y + c) / (2.0 * y + b - d);
            if (y - y_prev).abs() < 1e-12 {
                let dy_pre_fee = self.reserve_y - y;
                return Ok(dy_pre_fee * (1.0 - self.fee));
            }
        }
        Err(StableSwapError::DidNotConverge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_equilibrium_invariant_equals_2x() {
        let pool = StableSwapPool::new(1_000.0, 1_000.0, 100.0, 0.0004);
        let d = pool.invariant().unwrap();
        assert!((d - 2_000.0).abs() < 1e-6);
    }

    #[test]
    fn small_trade_low_slippage_for_high_a() {
        let pool_high_a = StableSwapPool::new(1_000_000.0, 1_000_000.0, 1000.0, 0.0004);
        let pool_low_a = StableSwapPool::new(1_000_000.0, 1_000_000.0, 10.0, 0.0004);
        let dx = 1_000.0;
        let hi = pool_high_a.out_given_in_x_to_y(dx).unwrap();
        let lo = pool_low_a.out_given_in_x_to_y(dx).unwrap();
        // Higher A ⇒ closer to 1:1 for like-priced assets.
        assert!(hi > lo);
        assert!(hi > 0.999 * dx);
    }
}
