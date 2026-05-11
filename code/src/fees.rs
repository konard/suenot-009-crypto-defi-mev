//! Gas and protocol-fee economics.
//!
//! On Ethereum-style chains, the marginal cost of a tx is
//!     gas_used · (base_fee + priority_fee).
//! Searchers competing for the same MEV opportunity bid up `priority_fee`
//! until expected profit ≈ 0. The `EffectiveFee` helper rolls these
//! components into a single number for back-of-envelope analysis.

#[derive(Debug, Clone, Copy)]
pub struct EffectiveFee {
    pub gas_used: u64,
    /// Wei per gas.
    pub base_fee: u64,
    /// Wei per gas (tip to validator/builder).
    pub priority_fee: u64,
    /// Native-token price in quote currency (e.g. ETH/USD).
    pub native_price: f64,
}

impl EffectiveFee {
    /// Total fee in native units (wei).
    pub fn total_wei(self) -> u128 {
        let per_gas = (self.base_fee + self.priority_fee) as u128;
        per_gas * self.gas_used as u128
    }

    /// Total fee in quote currency (USD).
    pub fn total_quote(self) -> f64 {
        let wei = self.total_wei() as f64;
        let eth = wei / 1e18;
        eth * self.native_price
    }

    /// Breakeven priority fee (wei/gas) that exactly absorbs `gross_profit_quote`.
    pub fn breakeven_priority_fee(self, gross_profit_quote: f64) -> u128 {
        if gross_profit_quote <= 0.0 || self.gas_used == 0 || self.native_price <= 0.0 {
            return 0;
        }
        let eth_profit = gross_profit_quote / self.native_price;
        let wei_budget = eth_profit * 1e18;
        let after_base = wei_budget - (self.base_fee as f64 * self.gas_used as f64);
        if after_base <= 0.0 {
            return 0;
        }
        (after_base / self.gas_used as f64) as u128
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakeven_caps_priority_fee() {
        let fee = EffectiveFee {
            gas_used: 200_000,
            base_fee: 20_000_000_000,    // 20 gwei
            priority_fee: 0,
            native_price: 2_500.0,
        };
        // Gross profit $100 ⇒ breakeven tip > 0 and finite.
        let bf = fee.breakeven_priority_fee(100.0);
        assert!(bf > 0);
    }

    #[test]
    fn breakeven_zero_when_no_profit() {
        let fee = EffectiveFee {
            gas_used: 200_000,
            base_fee: 20_000_000_000,
            priority_fee: 0,
            native_price: 2_500.0,
        };
        assert_eq!(fee.breakeven_priority_fee(0.0), 0);
    }
}
