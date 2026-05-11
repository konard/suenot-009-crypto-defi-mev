//! Optimal arbitrage between two constant-product AMMs.
//!
//! Setup: two pools `A` and `B` quote the same pair `(X, Y)` at different
//! prices. We borrow `dx` of X, sell into pool A receiving `dy_A` of Y,
//! then sell that `dy_A` into pool B receiving `dx'` of X. Profit is
//! `dx' - dx - gas`. The maximisation problem is:
//!
//! ```text
//! max over dx of  dx_back(dx) - dx
//! ```
//!
//! where `dx_back(dx)` is the V2 swap function composed twice. Both pools
//! have fee `f`. The closed-form optimum is:
//!
//! ```text
//! dx_opt = ( sqrt(alpha * beta) - x_A * y_B / gamma ) / gamma
//! ```
//!
//! with `alpha = x_A * y_A * gamma`, `beta = x_B * y_B * gamma`,
//! `gamma = (1 - f)^2`.
//!
//! See chapter section 9.3 for a derivation.

use crate::uniswap_v2::ConstantProductPool;

/// Closed-form optimal arbitrage size and expected profit between two pools.
///
/// Convention: trade direction is X→Y in pool A and Y→X in pool B.
/// Returns `(dx_optimal, profit_in_x)`. If no positive-EV trade exists,
/// returns `(0.0, 0.0)`.
pub fn two_pool_optimal(
    pool_a: &ConstantProductPool,
    pool_b: &ConstantProductPool,
) -> (f64, f64) {
    debug_assert!((pool_a.fee - pool_b.fee).abs() < 1e-12);
    let f = pool_a.fee;
    let gamma = (1.0 - f).powi(2);
    let xa = pool_a.reserve_x;
    let ya = pool_a.reserve_y;
    let xb = pool_b.reserve_x;
    let yb = pool_b.reserve_y;

    // No-arb if marginal prices already aligned.
    if (ya / xa - yb / xb).abs() / (ya / xa) < 1e-12 {
        return (0.0, 0.0);
    }

    let inside = xa * ya * xb * yb * gamma;
    if inside <= 0.0 {
        return (0.0, 0.0);
    }
    let dx_opt = (inside.sqrt() - xa * yb) / (yb + xa * (1.0 - f));
    // Numerical fallback: if the closed form gives negative size, search.
    if dx_opt <= 0.0 {
        return numerical_search(pool_a, pool_b);
    }

    let profit = simulate_profit(pool_a, pool_b, dx_opt);
    if profit <= 0.0 {
        // Try the reverse direction (Y→X in A, X→Y in B).
        return numerical_search(pool_a, pool_b);
    }
    (dx_opt, profit)
}

/// Fall-back numerical optimiser when the analytic formula doesn't apply
/// (different fees, three-pool legs, etc.). Uses golden-section search.
pub fn numerical_search(
    pool_a: &ConstantProductPool,
    pool_b: &ConstantProductPool,
) -> (f64, f64) {
    let try_dir = |a: &ConstantProductPool, b: &ConstantProductPool| -> (f64, f64) {
        let upper = a.reserve_x * 0.5;
        let phi = (5.0_f64.sqrt() - 1.0) / 2.0;
        let (mut lo, mut hi) = (1e-9, upper);
        let mut x1 = hi - phi * (hi - lo);
        let mut x2 = lo + phi * (hi - lo);
        let f1 = |x| simulate_profit(a, b, x);
        for _ in 0..200 {
            if f1(x1) < f1(x2) {
                lo = x1;
                x1 = x2;
                x2 = lo + phi * (hi - lo);
            } else {
                hi = x2;
                x2 = x1;
                x1 = hi - phi * (hi - lo);
            }
            if (hi - lo) < 1e-9 {
                break;
            }
        }
        let dx = 0.5 * (lo + hi);
        let p = f1(dx);
        (dx, p)
    };
    let (d1, p1) = try_dir(pool_a, pool_b);
    let (d2, p2) = try_dir(pool_b, pool_a);
    if p1 >= p2 && p1 > 0.0 {
        (d1, p1)
    } else if p2 > 0.0 {
        // Note: in direction B→A we return dx in B's units. Caller must know.
        (d2, p2)
    } else {
        (0.0, 0.0)
    }
}

fn simulate_profit(a: &ConstantProductPool, b: &ConstantProductPool, dx: f64) -> f64 {
    if dx <= 0.0 {
        return 0.0;
    }
    let Ok(dy) = a.out_given_in_x_to_y(dx) else { return 0.0 };
    let Ok(dx_back) = b.out_given_in_y_to_x(dy) else { return 0.0 };
    dx_back - dx
}

/// Triangular arbitrage across three pools `(X,Y), (Y,Z), (Z,X)`.
/// Returns the cycle profit (in X) when starting from `dx_in` X tokens.
pub struct TriangularArb<'a> {
    pub xy: &'a ConstantProductPool,
    pub yz: &'a ConstantProductPool,
    pub zx: &'a ConstantProductPool,
}

impl<'a> TriangularArb<'a> {
    pub fn cycle_profit(&self, dx_in: f64) -> f64 {
        let Ok(dy) = self.xy.out_given_in_x_to_y(dx_in) else { return 0.0 };
        let Ok(dz) = self.yz.out_given_in_x_to_y(dy) else { return 0.0 };
        let Ok(dx_out) = self.zx.out_given_in_x_to_y(dz) else { return 0.0 };
        dx_out - dx_in
    }

    /// Golden-section search for optimal `dx_in`.
    pub fn optimal_input(&self, max_input: f64) -> (f64, f64) {
        let phi = (5.0_f64.sqrt() - 1.0) / 2.0;
        let (mut lo, mut hi) = (1e-9, max_input);
        let mut x1 = hi - phi * (hi - lo);
        let mut x2 = lo + phi * (hi - lo);
        for _ in 0..200 {
            if self.cycle_profit(x1) < self.cycle_profit(x2) {
                lo = x1;
                x1 = x2;
                x2 = lo + phi * (hi - lo);
            } else {
                hi = x2;
                x2 = x1;
                x1 = hi - phi * (hi - lo);
            }
            if (hi - lo) < 1e-9 {
                break;
            }
        }
        let dx = 0.5 * (lo + hi);
        (dx, self.cycle_profit(dx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrage_profit_positive_when_prices_diverge() {
        // Pool A: 100 X / 10_000 Y  ⇒  price 100 Y per X
        // Pool B: 100 X / 11_000 Y  ⇒  price 110 Y per X
        let a = ConstantProductPool::new(100.0, 10_000.0, 0.003).unwrap();
        let b = ConstantProductPool::new(100.0, 11_000.0, 0.003).unwrap();
        let (_dx, profit) = numerical_search(&a, &b);
        assert!(profit > 0.0);
    }

    #[test]
    fn arbitrage_zero_when_prices_equal() {
        let a = ConstantProductPool::new(100.0, 10_000.0, 0.003).unwrap();
        let b = ConstantProductPool::new(50.0, 5_000.0, 0.003).unwrap();
        let (_dx, profit) = numerical_search(&a, &b);
        // Both pools price X at 100 Y; only fees can be lost.
        assert!(profit <= 0.0 || profit < 1e-6);
    }

    #[test]
    fn triangular_arb_returns_zero_for_consistent_prices() {
        // Three pools where X/Y · Y/Z · Z/X = 1 exactly ⇒ no arb (only fee loss).
        let xy = ConstantProductPool::new(1_000.0, 2_000.0, 0.003).unwrap();
        let yz = ConstantProductPool::new(2_000.0, 4_000.0, 0.003).unwrap();
        let zx = ConstantProductPool::new(4_000.0, 1_000.0, 0.003).unwrap();
        let tri = TriangularArb { xy: &xy, yz: &yz, zx: &zx };
        assert!(tri.cycle_profit(1.0) < 0.0);
    }
}
