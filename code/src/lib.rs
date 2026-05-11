//! Crypto DeFi & MEV — companion code for Chapter 9.
//!
//! This crate provides reference implementations of:
//! - Constant-product and concentrated-liquidity AMMs (Uniswap V2 / V3 math).
//! - StableSwap (Curve) invariant for like-priced assets.
//! - Optimal two-pool arbitrage and triangular arbitrage solvers.
//! - Sandwich attack simulator with EV decomposition.
//! - Flash-loan composability primitives.
//! - On-chain risk metrics: impermanent loss, liquidation distance.
//!
//! All modules are pure functions over fixed-point u128 / f64 math; no RPC or
//! network IO. The intent is to make the financial logic auditable in unit
//! tests rather than to ship a production trading bot.

pub mod uniswap_v2;
pub mod uniswap_v3;
pub mod curve;
pub mod arbitrage;
pub mod sandwich;
pub mod flash_loan;
pub mod risk;
pub mod fees;

pub use uniswap_v2::{ConstantProductPool, V2SwapError};
pub use uniswap_v3::{ConcentratedPool, Tick};
pub use curve::StableSwapPool;
pub use arbitrage::{two_pool_optimal, TriangularArb};
pub use sandwich::{SandwichSimulator, SandwichOutcome};
pub use flash_loan::{FlashLoanPlan, FlashLoanError};
pub use risk::{impermanent_loss, liquidation_distance, HealthFactor};
pub use fees::EffectiveFee;
