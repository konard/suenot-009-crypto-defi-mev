//! Flash-loan composability primitives.
//!
//! A flash loan is an atomic borrow-and-repay sequence: in a single
//! transaction the protocol lends `L` tokens, the caller executes an
//! arbitrary callback, and the protocol asserts that `L · (1 + fee)` is
//! returned by the end. If not, the entire transaction reverts.
//!
//! The relevant invariant for strategy design:
//!     terminal_balance ≥ L · (1 + fee)   ⇒   gross_pnl ≥ 0
//! Net PnL adds gas: `net = gross − gas`.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FlashLoanError {
    #[error("loan would not repay: terminal {terminal:?} < required {required:?}")]
    Insolvent { terminal: u64, required: u64 },
    #[error("flash loan fee must be in [0, 1)")]
    InvalidFee,
}

/// A simulated flash-loan plan composed of pure functions.
#[derive(Debug, Clone)]
pub struct FlashLoanPlan {
    pub principal: f64,
    pub fee: f64,
    pub gas_cost: f64,
}

impl FlashLoanPlan {
    pub fn new(principal: f64, fee: f64, gas_cost: f64) -> Result<Self, FlashLoanError> {
        if !(0.0..1.0).contains(&fee) {
            return Err(FlashLoanError::InvalidFee);
        }
        Ok(Self { principal, fee, gas_cost })
    }

    /// Amount the strategy must hold at callback end to pass the assertion.
    pub fn required_repayment(&self) -> f64 {
        self.principal * (1.0 + self.fee)
    }

    /// Net profit given the strategy's gross terminal balance.
    /// Returns `Err` if the loan cannot be repaid (in reality, the tx reverts).
    pub fn net_profit(&self, terminal_balance: f64) -> Result<f64, FlashLoanError> {
        let need = self.required_repayment();
        if terminal_balance < need {
            return Err(FlashLoanError::Insolvent {
                terminal: terminal_balance as u64,
                required: need as u64,
            });
        }
        Ok(terminal_balance - need - self.gas_cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insolvent_reverts() {
        let plan = FlashLoanPlan::new(1_000.0, 0.0009, 5.0).unwrap();
        let err = plan.net_profit(999.0).unwrap_err();
        assert!(matches!(err, FlashLoanError::Insolvent { .. }));
    }

    #[test]
    fn profitable_after_repayment_and_gas() {
        let plan = FlashLoanPlan::new(1_000.0, 0.0009, 5.0).unwrap();
        // Strategy ended with 1_050 in the same token.
        let net = plan.net_profit(1_050.0).unwrap();
        assert!(net > 0.0);
        // Sanity: gross profit 50, minus 1000·0.0009 = 0.9 fee minus 5 gas.
        assert!((net - (50.0 - 0.9 - 5.0)).abs() < 1e-9);
    }
}
