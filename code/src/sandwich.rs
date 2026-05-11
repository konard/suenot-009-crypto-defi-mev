//! Sandwich-attack simulator.
//!
//! Threat model: a searcher observes a victim transaction "buy `dy_victim` of Y
//! with at most `dx_max` of X, with slippage tolerance `s`". The searcher
//! front-runs with `dx_front` X→Y, lets the victim execute, then back-runs
//! Y→X to capture the price-impact gap.
//!
//! Outputs:
//!   - whether the victim still fits within their slippage tolerance after
//!     the front-run (otherwise the sandwich reverts and the searcher pays
//!     gas with no upside);
//!   - the searcher's profit, net of gas;
//!   - the victim's *additional* loss due to being sandwiched.

use crate::uniswap_v2::ConstantProductPool;

#[derive(Debug, Clone, Copy)]
pub struct VictimTrade {
    /// Maximum input the victim is willing to spend.
    pub max_input_x: f64,
    /// Minimum Y they accept (slippage tolerance baked in).
    pub min_output_y: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct SandwichOutcome {
    pub front_in_x: f64,
    pub front_out_y: f64,
    pub victim_out_y: f64,
    pub back_in_y: f64,
    pub back_out_x: f64,
    /// `back_out_x − front_in_x − gas_cost`. Negative ⇒ no attack.
    pub searcher_profit_x: f64,
    /// Y the victim *would have* received in absence of the sandwich,
    /// minus what they actually received.
    pub victim_loss_y: f64,
    pub reverted: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SandwichSimulator {
    pub gas_cost_x: f64,
}

impl SandwichSimulator {
    /// Evaluate a candidate sandwich with a given front-run input.
    pub fn simulate(
        &self,
        pool: &ConstantProductPool,
        victim: VictimTrade,
        front_in_x: f64,
    ) -> SandwichOutcome {
        // Counterfactual: victim alone.
        let mut counterfactual = *pool;
        let victim_out_y_counterfactual =
            counterfactual.apply_swap_x_to_y(victim.max_input_x).unwrap_or(0.0);

        // Real path: front-run, victim, back-run.
        let mut p = *pool;
        let front_out_y = match p.apply_swap_x_to_y(front_in_x) {
            Ok(y) => y,
            Err(_) => {
                return SandwichOutcome {
                    front_in_x,
                    front_out_y: 0.0,
                    victim_out_y: 0.0,
                    back_in_y: 0.0,
                    back_out_x: 0.0,
                    searcher_profit_x: -self.gas_cost_x,
                    victim_loss_y: 0.0,
                    reverted: true,
                };
            }
        };
        let victim_out_y = p.apply_swap_x_to_y(victim.max_input_x).unwrap_or(0.0);

        // Victim slippage check.
        if victim_out_y < victim.min_output_y {
            return SandwichOutcome {
                front_in_x,
                front_out_y,
                victim_out_y: 0.0,
                back_in_y: 0.0,
                back_out_x: 0.0,
                searcher_profit_x: -self.gas_cost_x,
                victim_loss_y: 0.0,
                reverted: true,
            };
        }

        let back_in_y = front_out_y;
        let back_out_x = p.apply_swap_y_to_x(back_in_y).unwrap_or(0.0);
        let searcher_profit_x = back_out_x - front_in_x - self.gas_cost_x;
        let victim_loss_y = victim_out_y_counterfactual - victim_out_y;

        SandwichOutcome {
            front_in_x,
            front_out_y,
            victim_out_y,
            back_in_y,
            back_out_x,
            searcher_profit_x,
            victim_loss_y,
            reverted: false,
        }
    }

    /// Golden-section search for the searcher's optimal front-run amount.
    /// Bounded by the victim's slippage tolerance: too aggressive ⇒ revert.
    pub fn optimal(&self, pool: &ConstantProductPool, victim: VictimTrade) -> SandwichOutcome {
        let phi = (5.0_f64.sqrt() - 1.0) / 2.0;
        let (mut lo, mut hi) = (1e-12, pool.reserve_x);
        let mut x1 = hi - phi * (hi - lo);
        let mut x2 = lo + phi * (hi - lo);
        let f = |x| {
            let o = self.simulate(pool, victim, x);
            if o.reverted {
                f64::MIN
            } else {
                o.searcher_profit_x
            }
        };
        for _ in 0..300 {
            if f(x1) < f(x2) {
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
        self.simulate(pool, victim, dx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_victim(pool: &ConstantProductPool) -> VictimTrade {
        // Victim trade ≈ 1% of reserves, 2% slippage tolerance.
        let dx = pool.reserve_x * 0.01;
        let baseline = pool.out_given_in_x_to_y(dx).unwrap();
        VictimTrade { max_input_x: dx, min_output_y: baseline * 0.98 }
    }

    #[test]
    fn sandwich_extracts_positive_value_on_large_victim() {
        let pool = ConstantProductPool::new(1_000.0, 1_000_000.0, 0.003).unwrap();
        let v = VictimTrade {
            max_input_x: 50.0,                   // ~5% of reserves
            min_output_y: 30_000.0,              // very loose slippage
        };
        let sim = SandwichSimulator { gas_cost_x: 0.0 };
        let out = sim.optimal(&pool, v);
        assert!(!out.reverted);
        assert!(out.searcher_profit_x > 0.0);
        assert!(out.victim_loss_y > 0.0);
    }

    #[test]
    fn tight_slippage_kills_sandwich() {
        let pool = ConstantProductPool::new(1_000.0, 1_000_000.0, 0.003).unwrap();
        let mut v = sample_victim(&pool);
        // Tighten slippage to 0.01% — essentially no room for the front-run.
        let baseline = pool.out_given_in_x_to_y(v.max_input_x).unwrap();
        v.min_output_y = baseline * 0.9999;
        let sim = SandwichSimulator { gas_cost_x: 0.01 };
        let out = sim.optimal(&pool, v);
        // Either reverts or profit barely covers gas.
        assert!(out.reverted || out.searcher_profit_x <= 0.0);
    }
}
