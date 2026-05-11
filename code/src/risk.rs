//! On-chain risk metrics.
//!
//! 1. Impermanent loss (IL) — opportunity cost of LPing in a constant-product
//!    pool vs. holding the two assets.
//!
//!    IL(p) = 2 · √p / (1 + p) − 1,    p = price_now / price_at_deposit
//!
//!    p=1   ⇒ IL = 0
//!    p=2   ⇒ IL ≈ −5.72%
//!    p=4   ⇒ IL ≈ −20%
//!
//! 2. Liquidation distance for an over-collateralised CDP (Maker/Aave style):
//!
//!    HF = (collateral · price · LTV_max) / debt
//!    liquidation_price = debt / (collateral · LTV_max)
//!
//! 3. Health factor < 1 ⇒ the position can be liquidated.

#[derive(Debug, Clone, Copy)]
pub struct HealthFactor(pub f64);

impl HealthFactor {
    pub fn is_liquidatable(self) -> bool {
        self.0 < 1.0
    }
}

/// IL relative to "hold both tokens" baseline. Returns a non-positive number.
pub fn impermanent_loss(price_ratio: f64) -> f64 {
    if price_ratio <= 0.0 {
        return 0.0;
    }
    2.0 * price_ratio.sqrt() / (1.0 + price_ratio) - 1.0
}

/// Health factor for a single-collateral CDP.
///
/// * `collateral_amount` — units of the collateral asset.
/// * `collateral_price`  — quote-currency price per unit.
/// * `ltv_max`           — maximum loan-to-value ratio (e.g. 0.825 for ETH).
/// * `debt_amount`       — debt denominated in the quote currency.
pub fn health_factor(
    collateral_amount: f64,
    collateral_price: f64,
    ltv_max: f64,
    debt_amount: f64,
) -> HealthFactor {
    if debt_amount <= 0.0 {
        return HealthFactor(f64::INFINITY);
    }
    HealthFactor(collateral_amount * collateral_price * ltv_max / debt_amount)
}

/// Price at which the position is liquidated.
pub fn liquidation_distance(
    collateral_amount: f64,
    ltv_max: f64,
    debt_amount: f64,
    current_price: f64,
) -> f64 {
    if collateral_amount <= 0.0 || ltv_max <= 0.0 {
        return 0.0;
    }
    let p_liq = debt_amount / (collateral_amount * ltv_max);
    (current_price - p_liq) / current_price
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_at_par_is_zero() {
        assert!(impermanent_loss(1.0).abs() < 1e-15);
    }

    #[test]
    fn il_negative_otherwise() {
        for p in [0.5, 0.75, 1.5, 2.0, 4.0] {
            assert!(impermanent_loss(p) < 0.0);
        }
    }

    #[test]
    fn il_2x_close_to_neg_5_72_pct() {
        let il = impermanent_loss(2.0);
        assert!((il + 0.0572).abs() < 0.001);
    }

    #[test]
    fn liquidation_when_collateral_drops() {
        // 10 ETH at $2000, LTV 0.8, $10_000 debt.
        let hf = health_factor(10.0, 2_000.0, 0.8, 10_000.0);
        assert!(!hf.is_liquidatable());
        let hf_low = health_factor(10.0, 1_000.0, 0.8, 10_000.0);
        // 10 · 1000 · 0.8 / 10000 = 0.8 < 1 ⇒ liquidatable.
        assert!(hf_low.is_liquidatable());
    }
}
