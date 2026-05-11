//! Concentrated liquidity (Uniswap V3) math, simplified to one active tick.
//!
//! Inside a tick range `[p_a, p_b]` with virtual liquidity `L`, the pool
//! obeys the "shifted" constant-product invariant
//!     (x + L/√p_b) · (y + L·√p_a) = L²
//!
//! From this one derives:
//!     • the current price `p = (y + L·√p_a) / (x + L/√p_b)`
//!     • the maximum input `x_max` before the price exits the range.
//!
//! This module implements the closed-form swap-within-range; multi-tick
//! traversal is left out for brevity (it's a loop over ticks).

#[derive(Debug, Clone, Copy)]
pub struct Tick {
    /// Lower bound of the active range in raw price units (Y per X).
    pub price_lower: f64,
    /// Upper bound of the active range in raw price units (Y per X).
    pub price_upper: f64,
    /// Virtual liquidity `L`.
    pub liquidity: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ConcentratedPool {
    pub tick: Tick,
    /// Current price `p`; must lie in `[price_lower, price_upper]`.
    pub price: f64,
    pub fee: f64,
}

impl ConcentratedPool {
    pub fn new(tick: Tick, price: f64, fee: f64) -> Self {
        debug_assert!(price >= tick.price_lower && price <= tick.price_upper);
        debug_assert!((0.0..1.0).contains(&fee));
        Self { tick, price, fee }
    }

    /// Token-X (e.g. ETH) reserves implied by the V3 formula.
    pub fn virtual_x(&self) -> f64 {
        let l = self.tick.liquidity;
        let p = self.price.sqrt();
        let pb = self.tick.price_upper.sqrt();
        l * (pb - p) / (p * pb)
    }

    /// Token-Y (e.g. USDC) reserves implied by the V3 formula.
    pub fn virtual_y(&self) -> f64 {
        let l = self.tick.liquidity;
        let p = self.price.sqrt();
        let pa = self.tick.price_lower.sqrt();
        l * (p - pa)
    }

    /// Maximum X-input that keeps the price inside the active range.
    pub fn max_input_x(&self) -> f64 {
        let l = self.tick.liquidity;
        let p = self.price.sqrt();
        let pa = self.tick.price_lower.sqrt();
        l * (1.0 / pa - 1.0 / p)
    }

    /// Output Y for an X-input that stays in range. If the trade would
    /// exit the range, the returned value is the *partial* fill.
    pub fn swap_x_to_y(&mut self, dx: f64) -> f64 {
        let dx_eff = dx * (1.0 - self.fee);
        let l = self.tick.liquidity;
        let sqrt_p = self.price.sqrt();
        // sqrt_p_new = L · sqrt_p / (L + dx · sqrt_p)
        let sqrt_p_new = l * sqrt_p / (l + dx_eff * sqrt_p);
        let sqrt_p_lower = self.tick.price_lower.sqrt();
        let sqrt_p_new = sqrt_p_new.max(sqrt_p_lower);
        let dy = l * (sqrt_p - sqrt_p_new);
        self.price = sqrt_p_new * sqrt_p_new;
        dy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_reserves_consistent() {
        let pool = ConcentratedPool::new(
            Tick { price_lower: 1000.0, price_upper: 4000.0, liquidity: 1e6 },
            2_000.0,
            0.003,
        );
        let x = pool.virtual_x();
        let y = pool.virtual_y();
        // Implied price reconstructed from virtual reserves.
        let l = pool.tick.liquidity;
        let pa = pool.tick.price_lower.sqrt();
        let pb = pool.tick.price_upper.sqrt();
        let p_rec = (y + l * pa) / (x + l / pb);
        assert!((p_rec - pool.price).abs() / pool.price < 1e-9);
    }

    #[test]
    fn swap_decreases_price() {
        let mut pool = ConcentratedPool::new(
            Tick { price_lower: 1000.0, price_upper: 4000.0, liquidity: 1e6 },
            2_000.0,
            0.003,
        );
        let p0 = pool.price;
        let _ = pool.swap_x_to_y(10.0);
        assert!(pool.price < p0);
    }
}
