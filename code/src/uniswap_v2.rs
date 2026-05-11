//! Constant-product AMM (x * y = k) — Uniswap V2 math.
//!
//! Given reserves `(x, y)` and a swap fee `f ∈ [0,1)`, swapping `dx` of token X
//! into the pool returns:
//!     dy = (y · dx · (1 − f)) / (x + dx · (1 − f))
//!
//! The function is monotonically increasing and strictly concave in `dx`, which
//! makes optimal-arbitrage closed-form solvable (see [`crate::arbitrage`]).
//!
//! Price impact (slippage) of a swap of size `dx` is
//!     slip = dy_quote(dx) / dy_quote(0+) − 1   (always ≤ 0)
//! where `dy_quote(0+) = (y/x)(1−f)` is the mid-price.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum V2SwapError {
    #[error("input amount must be positive")]
    NonPositiveInput,
    #[error("reserves must be positive")]
    EmptyReserves,
    #[error("fee must be in [0, 1)")]
    InvalidFee,
}

/// A two-token constant-product pool.
///
/// Reserves are stored as `f64` for clarity. In production one uses `u128`
/// with full-width 256-bit multiplication; the module
/// [`crate::uniswap_v2::int_math`] mirrors the integer routine for tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstantProductPool {
    pub reserve_x: f64,
    pub reserve_y: f64,
    pub fee: f64,
}

impl ConstantProductPool {
    /// Creates a pool. Returns an error on degenerate inputs.
    pub fn new(reserve_x: f64, reserve_y: f64, fee: f64) -> Result<Self, V2SwapError> {
        if reserve_x <= 0.0 || reserve_y <= 0.0 {
            return Err(V2SwapError::EmptyReserves);
        }
        if !(0.0..1.0).contains(&fee) {
            return Err(V2SwapError::InvalidFee);
        }
        Ok(Self { reserve_x, reserve_y, fee })
    }

    /// Invariant `k = x · y` (constant across fee-less trades).
    pub fn k(&self) -> f64 {
        self.reserve_x * self.reserve_y
    }

    /// Spot price of X in units of Y (no slippage, no fee).
    pub fn mid_price(&self) -> f64 {
        self.reserve_y / self.reserve_x
    }

    /// Effective marginal price of X→Y including fee.
    pub fn marginal_price_x_to_y(&self) -> f64 {
        self.mid_price() * (1.0 - self.fee)
    }

    /// Swap `dx` of token X in, get the amount of Y out.
    pub fn out_given_in_x_to_y(&self, dx: f64) -> Result<f64, V2SwapError> {
        if dx <= 0.0 {
            return Err(V2SwapError::NonPositiveInput);
        }
        let dx_eff = dx * (1.0 - self.fee);
        Ok(self.reserve_y * dx_eff / (self.reserve_x + dx_eff))
    }

    /// Swap `dy` of token Y in, get the amount of X out (symmetric).
    pub fn out_given_in_y_to_x(&self, dy: f64) -> Result<f64, V2SwapError> {
        if dy <= 0.0 {
            return Err(V2SwapError::NonPositiveInput);
        }
        let dy_eff = dy * (1.0 - self.fee);
        Ok(self.reserve_x * dy_eff / (self.reserve_y + dy_eff))
    }

    /// Required input `dx` to receive an exact `dy` out of the pool.
    pub fn in_given_out_x_to_y(&self, dy: f64) -> Result<f64, V2SwapError> {
        if dy <= 0.0 {
            return Err(V2SwapError::NonPositiveInput);
        }
        if dy >= self.reserve_y {
            return Err(V2SwapError::EmptyReserves);
        }
        let num = self.reserve_x * dy;
        let den = (self.reserve_y - dy) * (1.0 - self.fee);
        Ok(num / den)
    }

    /// Apply a swap mutably and return the output amount.
    pub fn apply_swap_x_to_y(&mut self, dx: f64) -> Result<f64, V2SwapError> {
        let dy = self.out_given_in_x_to_y(dx)?;
        self.reserve_x += dx;
        self.reserve_y -= dy;
        Ok(dy)
    }

    /// Apply a swap mutably and return the output amount.
    pub fn apply_swap_y_to_x(&mut self, dy: f64) -> Result<f64, V2SwapError> {
        let dx = self.out_given_in_y_to_x(dy)?;
        self.reserve_y += dy;
        self.reserve_x -= dx;
        Ok(dx)
    }

    /// Slippage of an X→Y swap, defined as
    ///     `executed_price / mid_price − 1`
    /// where `executed_price = dy_out / dx_in` denominated in Y per X.
    /// Returns a non-positive number; large `|dx|` ⇒ more negative slippage.
    pub fn slippage_x_to_y(&self, dx: f64) -> Result<f64, V2SwapError> {
        let dy = self.out_given_in_x_to_y(dx)?;
        let executed = dy / dx;
        let mid = self.marginal_price_x_to_y();
        Ok(executed / mid - 1.0)
    }
}

/// Integer-arithmetic mirror used in tests. Saturating wrappers protect
/// against overflow for realistic on-chain magnitudes.
pub mod int_math {
    /// Computes `floor(y · dx · (10000 − fee_bps) / (x · 10000 + dx · (10000 − fee_bps)))`
    /// without overflowing `u128` for reserves up to ~2^96.
    pub fn out_given_in(
        reserve_in: u128,
        reserve_out: u128,
        amount_in: u128,
        fee_bps: u32,
    ) -> u128 {
        let fee_complement = 10_000u128 - fee_bps as u128;
        let amount_in_with_fee = amount_in.saturating_mul(fee_complement);
        let numerator = amount_in_with_fee.saturating_mul(reserve_out);
        let denominator = reserve_in
            .saturating_mul(10_000)
            .saturating_add(amount_in_with_fee);
        if denominator == 0 {
            0
        } else {
            numerator / denominator
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invariant_grows_with_fee() {
        let mut pool = ConstantProductPool::new(1_000.0, 1_000.0, 0.003).unwrap();
        let k_before = pool.k();
        pool.apply_swap_x_to_y(10.0).unwrap();
        // Fee accrues to LPs ⇒ k strictly grows.
        assert!(pool.k() > k_before);
    }

    #[test]
    fn round_trip_loses_to_fees() {
        let mut pool = ConstantProductPool::new(1_000.0, 1_000.0, 0.003).unwrap();
        let dx = 5.0;
        let dy = pool.apply_swap_x_to_y(dx).unwrap();
        let dx_back = pool.apply_swap_y_to_x(dy).unwrap();
        assert!(dx_back < dx, "round trip must lose to fees");
    }

    #[test]
    fn slippage_is_non_positive() {
        let pool = ConstantProductPool::new(1_000.0, 1_000.0, 0.003).unwrap();
        assert!(pool.slippage_x_to_y(50.0).unwrap() <= 0.0);
    }

    #[test]
    fn integer_routine_matches_float_within_1bp() {
        // 30 bps fee, equal reserves.
        let out_int =
            int_math::out_given_in(1_000_000_000_000, 1_000_000_000_000, 1_000_000, 30);
        let pool = ConstantProductPool::new(1e12, 1e12, 0.003).unwrap();
        let out_float = pool.out_given_in_x_to_y(1e6).unwrap();
        let rel = (out_int as f64 - out_float).abs() / out_float;
        assert!(rel < 1e-4);
    }

    #[test]
    fn exact_out_inverts_exact_in() {
        let pool = ConstantProductPool::new(2_000.0, 5_000.0, 0.003).unwrap();
        let dy = 100.0;
        let dx = pool.in_given_out_x_to_y(dy).unwrap();
        let dy_round = pool.out_given_in_x_to_y(dx).unwrap();
        assert!((dy_round - dy).abs() < 1e-9);
    }
}
