# Chapter 9 — Crypto, DeFi and MEV, the Simple Version

Imagine a stock exchange that never closes, has no employees, no clearing house, no margin desk — and where every order, fill and balance lives in plain sight on a public ledger. That is DeFi: the order book has been replaced by a smart contract, and the matching engine is a piece of code that you can read, fork, and trade against.

This chapter takes you from "how does a swap actually work" all the way to "how do bots earn money from being one transaction faster than you." We keep the math, but we promise an analogy for every formula.

---

## Why crypto trading is different

**Analogy.** On Nasdaq, your order goes into a private queue inside a data centre in New Jersey. On Ethereum, your order goes into a *public waiting room* called the mempool. Everyone can see it before it executes. Imagine putting your buy order on a billboard for 12 seconds before the trade goes through — and somebody can pay extra to jump in front of you.

That public waiting room creates a whole new sport called **MEV** (Maximal Extractable Value): squeezing money out of the *order* in which transactions are placed in a block.

Three other quirks make crypto markets unique:

- **Atomicity.** A whole strategy can succeed or fail as a single unit. You can borrow a million dollars, do five trades, and pay it back — all in one transaction. If any step fails, none of it happened. This is the *flash loan*.
- **Composability.** Every protocol can call every other protocol. It is like LEGO: you can stack Uniswap + Aave + Curve in one contract.
- **Gas.** Every operation costs a fee that you pay to the validator. Bots fight a public auction for the right to be included first.

---

## How an Automated Market Maker actually works

There is no order book. Instead there is a pool with two piles of coins, say $x$ tokens of X and $y$ tokens of Y. The pool obeys one rule:

$$
x \cdot y = k
$$

The product of the two reserves is constant. If you put in some X, you must take out enough Y to keep the product the same.

**Analogy.** Think of two buckets connected by a seesaw. Push water into one bucket and the other rises — but the *product* of the two water levels is fixed. The deeper the pool, the less the levels move for the same amount of water. That is why big pools have less slippage.

If you swap $\Delta x$ in (after a fee $f$, usually 0.3%), you get out:

$$
\Delta y = \frac{y \cdot (1-f) \cdot \Delta x}{x + (1-f) \cdot \Delta x}
$$

Three things to remember:

1. The bigger your trade relative to the pool, the worse the price you get.
2. There is no "best bid / best ask." There is one price for any given trade size, set by the formula.
3. Whoever supplies the two buckets earns the fee — they are called *liquidity providers*.

Newer designs are tweaks on this:

- **Uniswap V3** lets liquidity providers concentrate their tokens around a chosen price range. Tighter range, more fees, more risk.
- **Curve** uses a different curve that is almost flat near the equal-reserve point. Great for stablecoins where 1 USDC really should equal 1 USDT.

---

## MEV: the gas auction for transaction order

When you submit a swap, it sits in the mempool. A "searcher" bot watches the mempool, computes whether your trade creates an opportunity, and bids for the right to insert their own trade *just before* yours.

**Analogy.** A florist is about to bid on the only remaining bouquet at an auction. Another bidder sees this in the catalogue, runs to the auction first, buys the bouquet at the listed price, and sells it to the florist a minute later for more. The original bidder still got the bouquet; they just paid more. That is a *sandwich attack*.

There are three main MEV games:

- **Arbitrage.** Two pools price the same token differently. You buy cheap in one, sell expensive in the other, all in one transaction. We derive a closed-form formula for the optimal trade size in the chapter — it depends on the reserves of both pools and the fee.
- **Sandwich.** You see a victim's buy order. You front-run it (push the price up), let them fill at the worse price, and immediately sell (back-run) to capture the difference. The math is constrained by the victim's slippage tolerance.
- **Liquidation.** Someone's loan becomes undercollateralised. You repay part of their debt and claim their collateral at a discount.

The competition is fierce. If you find a $100 arb, you bid $99 in priority fee to make sure you win. In equilibrium, most of the value goes to validators, not searchers. This is exactly the **Kelly criterion** from Chapter 3, applied to a public auction with adversaries.

---

## Flash loans: free money, almost

You can borrow any amount of any token, with no collateral, as long as you pay it back in the same transaction. If you do not, the whole transaction is reverted as if it never happened — so the lender has no risk.

**Analogy.** A bank lets you walk in, grab $50 million in cash, do whatever you want for one minute, and walk out — but if you do not put the exact amount back before the minute is up, time rewinds and the visit never occurred. No one even remembers you tried.

This sounds magical, but the catch is that whatever you do with the money has to *generate profit by itself* — usually by exploiting a price difference or a flaw in another contract. Famous attacks (bZx, Harvest, Mango Markets) used flash loans not to "trade" but to temporarily manipulate an oracle and trick another protocol into a bad price.

For a quant, flash loans are useful for two legitimate things:

1. **Capital efficiency.** You can run a $10M arb strategy with no inventory at risk.
2. **Atomic refinancing.** Switch your loan from Aave to Compound in one transaction.

---

## On-chain risk: why a wallet is not a margin account

On a CEX, if you blow up, you call support. On-chain, *the smart contract is the law*. There is no support.

The two big numbers you must internalise:

- **Health factor.** $\text{HF} = (\text{collateral value} \times \text{liquidation ratio}) / \text{debt value}$. When HF falls below 1, anybody on the planet can liquidate you. Usually they take a 5–10% discount on your collateral as their reward.
- **Impermanent loss.** When you provide liquidity to an AMM and the price moves, you end up with *less* value than if you had just held the two tokens. It is not a true loss — only versus the buy-and-hold alternative. But for volatile pairs it can dwarf the fees you earn.

**Analogy for impermanent loss.** Imagine a market-maker on the NYSE who is forced to *always quote both sides* at the current mid. If the stock rallies, she keeps selling into the rally and runs out of inventory at low prices. Same thing in an AMM: as Y appreciates, the pool keeps selling Y for X, and the LP wakes up holding mostly the loser.

There is a precise formula for the expected loss, called **LVR** (Loss-Versus-Rebalancing): roughly $\sigma^2 / 8$ per unit time. The more volatile the asset, the more you bleed.

---

## What the Rust code does

The `code/` folder is a small library that lets you simulate everything in this chapter on your laptop:

- `uniswap_v2.rs` — the basic $x \cdot y = k$ pool. Swap, mid-price, slippage. Plus an integer-only mirror so you can compare against the on-chain version.
- `uniswap_v3.rs` — concentrated liquidity for a single tick range.
- `curve.rs` — Curve's StableSwap invariant, solved by Newton iteration.
- `arbitrage.rs` — closed-form two-pool arb, golden-section search, triangular arb.
- `sandwich.rs` — sandwich simulator that respects the victim's slippage tolerance.
- `flash_loan.rs` — flash-loan accounting (borrow + fee + repay).
- `risk.rs` — health factor, impermanent loss, liquidation distance.
- `fees.rs` — EIP-1559 gas math and the breakeven priority fee.

There are 22 unit tests and 6 Criterion benchmarks. Run `cargo test` and `cargo bench` from inside `code/` to play with them.

---

## Takeaway

DeFi is not a different financial system. It is the same financial system, but with the *order book replaced by code* and the *clearing house replaced by mathematics*. Once you accept that, every concept from Chapters 1–8 has an on-chain analogue: market making becomes liquidity provision, arbitrage becomes searching, margin calls become liquidations, and the trading floor becomes a 12-second-long public auction.

The opportunities are real. So is the competition. Read the contracts, do the math, and never trust a pool you have not simulated.
