# Chapter 9: Crypto Specifics — DeFi and MEV in Algorithmic Trading

## Introduction

The first eight chapters of this book live in the world of *centralised* market microstructure: limit order books, broker APIs, time-priority matching, regulated intermediaries. The crypto markets started out as a faithful copy of that model — Binance and Coinbase are structurally indistinguishable from NASDAQ from a quant's point of view — but in 2018 a parallel ecosystem emerged that obeys completely different rules. Trading no longer happens in a continuous order book matched by an exchange operator. It happens inside *smart contracts* — pieces of code deployed on a public blockchain that anyone can call, anyone can read, and that execute their pricing logic deterministically.

This is the world of *DeFi* — decentralised finance. From the quant's standpoint it raises three questions that the rest of this book has not had to answer:

1. **Price formation.** If there is no order book, who sets the price? The answer is a class of stateful functions called *Automated Market Makers* (AMMs). Their math is not Walrasian equilibrium but a single algebraic invariant, and it has surprising consequences for slippage, inventory risk, and the value extracted from liquidity providers.
2. **Order priority.** If there is no exchange operator to enforce time-priority, what determines whose trade lands first? The answer is *gas auctions*: every transaction comes with a bid for inclusion, and the validators producing blocks order them by that bid (modulo private orderflow). The economic value siphoned out of this auction by sophisticated participants — front-running, sandwiching, latency arbitrage in a public mempool — is called *Maximal Extractable Value* (MEV), and it now exceeds a billion dollars per year on Ethereum alone.
3. **Risk decomposition.** A position on a centralised exchange is mostly subject to market risk. A position in DeFi is subject to market risk plus *protocol risk* (the contract may be exploited), *oracle risk* (the price feed may be wrong), *liquidation risk* (over-collateralised loans get wiped at a programmatic threshold), and *bridge risk* (cross-chain hops are the single largest source of exploits in the history of the space). A quant who does not model these is not modelling the position.

This chapter is the bridge from a "TradFi" view of crypto — Binance candles and CCXT order books — to an "on-chain" view, where the unit of analysis is not a trade but a state transition on a blockchain. We will treat AMMs as continuous market makers with a closed-form impact function; we will treat MEV as a sequential auction game with imperfect information; and we will treat over-collateralised lending as a regime-switching process with a hard boundary at the liquidation price. The math is drawn from previous chapters — stochastic calculus from Chapter 1, microstructure from Chapter 2, game theory from Chapter 7, Kelly from Chapter 6 — but the *application* is new.

A working Rust crate (`code/`) accompanies the text. Every formula in this chapter has a counterpart function with unit tests; every strategy has a simulator. You should be able to clone the repository, run `cargo test`, and reproduce every number in the figures.

> **Prerequisites.** Chapters 1 (stochastic calculus, geometric Brownian motion), 2 (limit order book, slippage), 6 (Kelly, log-optimal sizing), 7 (auctions, sealed-bid mechanics) are assumed throughout. Chapter 3 (portfolio optimisation, mean-variance) is helpful for §9.5.

---

## 9.1 Architecture of DeFi for Quants

### 9.1.1 What a blockchain actually is, from a market-maker's seat

A blockchain is, for our purposes, three things:

1. **An append-only, replicated state machine.** There is a global mapping `state: address → bytes` updated by transactions. State transitions are deterministic — given the same input, every node computes the same output — and final after a few blocks of confirmation.
2. **A public mempool of pending transactions.** Before inclusion, transactions are visible to anyone running a node. This is the *single most important fact* for MEV: it converts every public DEX transaction into a free option for any searcher who can react fast enough.
3. **An auction over block space.** The validator (Ethereum) or proposer (most other chains) chooses which transactions to include and in what order, weighted by the fee each transaction pays. This auction is the substrate on which MEV runs.

The latency budget of a quant trading on-chain is therefore very different from one trading on Binance. On Binance you compete on round-trip TCP latency to Tokyo, in microseconds. On Ethereum you compete on (a) connection latency to a relay (Flashbots, Eden, BloXroute) — milliseconds, and (b) the spot bid in the block-space auction — measured in priority fee per gas. The first you control with infrastructure; the second you control with strategy.

The state itself is updated in *blocks*, which arrive at a fixed cadence (12 seconds on Ethereum, sub-second on Solana, ~2 seconds on Arbitrum). Within a block, all transactions execute *atomically and sequentially*, which gives strategies a magical property unavailable in TradFi: **strategies can revert**. If a planned arbitrage doesn't clear after sending the front-leg, the back-leg's revert undoes the whole bundle and the searcher pays only gas. This single property reshapes the optimisation problem we will study in §9.3.

### 9.1.2 Anatomy of a DeFi position

A DeFi position is not a balance on an exchange. It is one or more of the following on-chain primitives:

- **ERC-20 token balances** — fungible tokens, the analog of "spot inventory."
- **AMM LP shares** — pro-rata claims on a liquidity pool. Holding LP shares is short volatility (more on this in §9.2.5).
- **CDPs (collateralised debt positions)** — borrow position in protocols like Aave or Maker. Always over-collateralised; a hard liquidation threshold lurks below.
- **Concentrated-liquidity NFTs** — Uniswap V3 positions, which are LP shares restricted to a price range. They earn higher fees but have leveraged IL inside the range.
- **Synthetic perp positions** — futures-like exposure on protocols such as dYdX, GMX, or Hyperliquid; structurally closer to traditional derivatives.
- **Yield-bearing wrappers** — `aTokens`, `stETH`, `cbETH` and similar. Quietly compound and have their own oracle quirks.

Each of these has a distinct PnL formula and a distinct risk vector. The first job of a DeFi quant is to *write down the position's mark-to-market function*. The second job is to *write down the conditions under which it can be liquidated, exploited, or de-pegged*.

### 9.1.3 The DEX landscape

There are now hundreds of DEX implementations, but their pricing functions cluster into a few families:

| Family | Examples | Pricing function |
|---|---|---|
| Constant-product (CPMM) | Uniswap V2, SushiSwap, PancakeSwap V2 | `x · y = k` |
| Concentrated liquidity | Uniswap V3, V4, PancakeSwap V3 | `(x + L/√p_b)(y + L·√p_a) = L²` inside a tick |
| StableSwap | Curve, Saddle, Ellipsis | `A · n^n · S + D = A · D · n^n + D^{n+1}/(n^n · ∏ x_i)` |
| Proactive market maker | DODO, Clipper | Piecewise: aggressive near oracle, defensive far from it |
| Weighted | Balancer | `∏ x_i^{w_i} = k`, generalised Cobb–Douglas |
| Liquidity Book | Trader Joe V2 | Discretised bins of constant-sum AMMs |
| Order-book hybrid | dYdX, Hyperliquid, ApeX | Off-chain matching, on-chain settlement |

We will treat the first three explicitly. The rest reduce to combinations of these in their swap math.

### 9.1.4 Costs that matter

Every DEX trade incurs at least four costs, of which only the first is visible on a TradFi exchange:

1. **Protocol fee.** 30 bps (Uniswap V2), 1/5/30/100 bps (V3 fee tiers), ~4 bps (Curve stableswaps). Paid to LPs, sometimes split with a DAO treasury.
2. **Price impact.** A function of swap size relative to liquidity. Derivable in closed form from the AMM invariant.
3. **Gas.** The cost of a single swap is dominated by gas at small sizes; this matters enormously for the Kelly-optimal trade size (§9.3.5).
4. **MEV tax.** Mostly invisible; estimated by the gap between the price you'd pay against an *uncongested* AMM and the price you actually receive after the bot ecosystem has had its say. Recent academic work puts the average sandwich tax at 5–15 bps on retail-sized swaps; for sizeable trades the variance is huge.

A correct backtest of an on-chain strategy must include all four. A backtest that uses only protocol fee will systematically overestimate edge by an order of magnitude.

> **Code reference.** `code/src/fees.rs` implements an `EffectiveFee` helper that converts the (gas_used, base_fee, priority_fee, native_price) tuple into a single quote-currency cost and computes the *breakeven priority fee* a searcher can bid given a known gross profit. We use this in §9.3.6.

---

## 9.2 Automated Market Makers

### 9.2.1 Constant-Product (Uniswap V2)

The simplest and still most important AMM. The pool's state is a pair of reserves $(x, y)$ of two ERC-20 tokens. After every trade, the *invariant*

$$
x \cdot y = k
$$

is preserved (up to fees). A swap of $\Delta x$ tokens of $X$ into the pool, in exchange for $\Delta y$ of $Y$, satisfies

$$
(x + (1-f)\Delta x)(y - \Delta y) = x \cdot y
$$

with $f$ the fee. Solving for $\Delta y$:

$$
\boxed{\Delta y = \frac{y \cdot (1-f)\Delta x}{x + (1-f)\Delta x}}.
$$

**Spot price and impact.** The spot price (no slippage, no fee) is $p = y/x$. The marginal price for an infinitesimal X→Y trade is $p \cdot (1-f)$. The *effective* price for a swap of size $\Delta x$ is the average

$$
\bar p (\Delta x) = \frac{\Delta y}{\Delta x} = \frac{y (1-f)}{x + (1-f)\Delta x}.
$$

The relative slippage — the gap between effective price and marginal price — is

$$
\text{slip}(\Delta x) = \frac{\bar p}{p(1-f)} - 1 = -\frac{(1-f)\Delta x}{x + (1-f)\Delta x}.
$$

For small trades $\Delta x \ll x$, slippage is approximately $-\Delta x / x$, i.e. linear in size and inversely proportional to pool depth.

**Comparison with order-book microstructure.** In Chapter 2 we defined Kyle's lambda $\lambda$ as the linear price impact coefficient. For a CPMM, the analogous coefficient is

$$
\lambda_{\text{cpmm}} = \frac{p}{x} = \frac{y}{x^2}.
$$

That is, the AMM behaves like a Kyle market with $\lambda$ inversely proportional to the dollar value of one side of the pool. This is a useful intuition: when LPs deposit more capital, the pool "sees" lower impact, exactly like a deeper order book.

**Code reference.** `ConstantProductPool` in `code/src/uniswap_v2.rs` implements all of the above, plus an integer-arithmetic mirror (`int_math::out_given_in`) that matches the on-chain bytecode. The unit test `integer_routine_matches_float_within_1bp` asserts they agree to <1 bp on realistic reserves.

```rust
let pool = ConstantProductPool::new(1_000.0, 1_000_000.0, 0.003)?;
let dy = pool.out_given_in_x_to_y(10.0)?;
let slippage = pool.slippage_x_to_y(10.0)?;
```

### 9.2.2 Concentrated Liquidity (Uniswap V3)

V2 is capital-inefficient: liquidity sits everywhere on the price curve from $0$ to $\infty$, but the vast majority of trades happen near the current price. V3 fixes this by allowing LPs to specify a *range* $[p_a, p_b]$ over which their capital is active. Within an active tick range, the pool obeys the *virtual* invariant

$$
\left(x + \frac{L}{\sqrt{p_b}}\right) \left(y + L \sqrt{p_a}\right) = L^2
$$

where $L$ is the *liquidity* parameter and $p = \sqrt{p_a} \cdot \sqrt{p_b}$ at the edges. As the price moves within $[p_a, p_b]$, the pool behaves identically to a V2 pool with reserves equal to the virtual quantities $x + L/\sqrt{p_b}$ and $y + L\sqrt{p_a}$. Outside the range, the LP is fully in one asset and earns no fees.

**Key V3 swap formula.** For a swap that stays inside a single tick, the post-swap price satisfies

$$
\sqrt{p_{\text{new}}} = \frac{L \sqrt{p}}{L + (1-f)\Delta x \sqrt{p}}
$$

and the output is

$$
\Delta y = L (\sqrt{p} - \sqrt{p_{\text{new}}}).
$$

This is implemented in `code/src/uniswap_v3.rs`. The test `swap_decreases_price` verifies monotonicity; `virtual_reserves_consistent` verifies that the implied virtual reserves reconstruct the current price.

**Concentrated IL.** The cost of capital efficiency is *leverage on impermanent loss*. A V3 position with range $[p_a, p_b]$ around current price $p_0$ behaves like a V2 position with $r = \sqrt{p_b/p_a}$-times more liquidity — but only inside the range. Outside it, the position has fully converted to the depreciated side. The closed-form IL of a V3 position is, when $p$ stays inside the range,

$$
\text{IL}_{V3}(p) = \frac{2 \sqrt{p / p_0} - (1 + p/p_0)}{1 + (p/p_0)} \cdot \frac{\sqrt{p_b/p_a}}{\sqrt{p_b/p_a} - 1}
$$

which converges to the V2 formula when the range is wide. For narrow ranges, IL is steep — concentrated LPs are essentially short volatility on the pair.

**The "JIT-LP" problem.** Within a single block, a sophisticated actor can deposit a huge V3 position, capture the fee of one large trade, and withdraw — all in one transaction. This *just-in-time liquidity* compresses fees for passive LPs and has reshaped V3 market making since 2022.

### 9.2.3 StableSwap (Curve)

For two assets that *should* trade close to par (USDC/USDT, stETH/ETH, WBTC/renBTC), CPMM is wasteful: it forces 50/50 inventory regardless of how strong the parity is. Curve's *StableSwap* invariant interpolates between the constant-sum AMM ($x + y = k$, perfect parity, zero slippage, infinite IL) and the constant-product AMM ($x \cdot y = k$, no parity assumption, smooth slippage). For an $n$-coin pool:

$$
A n^n \sum_i x_i + D = A D n^n + \frac{D^{n+1}}{n^n \prod_i x_i}
$$

The *amplification* coefficient $A$ controls the trade-off. Near equilibrium ($x_i$ all equal), the curve is almost flat — large trades have tiny slippage. Far from equilibrium, the curve bends to the constant-product asymptote, protecting LPs from being drained when a peg breaks.

There is no closed-form solution for $D$ or for the post-swap reserves; both are found by Newton iteration. `code/src/curve.rs` implements both; the test `at_equilibrium_invariant_equals_2x` verifies $D = 2x$ at equal reserves, and `small_trade_low_slippage_for_high_a` verifies that higher $A$ yields tighter execution near par.

**A quant's view of StableSwap.** When the peg holds, you can trade USDC↔USDT on Curve with effectively zero slippage and a 4 bp fee. When the peg breaks — March 2023 (USDC depeg), May 2022 (UST collapse), Sept 2022 (stETH discount) — Curve pools act like a CPMM rapidly, and the slippage curve has a sharp knee. Quants short the peg run their PnL through this knee: every basis point of widening pays them, and Curve's geometry is what defines "every basis point."

### 9.2.4 The Universal AMM Pricing Function

Despite the diversity above, all AMMs share a common structure: a *conserved bonding function* $\Phi(x, y) = k$ that defines a one-dimensional manifold of admissible reserves. A swap is a movement along the manifold. The marginal price is

$$
p = -\frac{\partial \Phi / \partial x}{\partial \Phi / \partial y}
$$

For CPMM: $\Phi = xy$ gives $p = y/x$.
For StableSwap: $\Phi$ is the curve invariant; $p$ depends on $x, y, A$.
For Balancer-style weighted: $\Phi = x^{w_1} y^{w_2}$ gives $p = (w_1/w_2)(y/x)$.

This unified view is useful for one practical reason: optimal routing across heterogenous pools reduces to convex optimisation over the *concatenated swap functions*, all of which are concave. This is why aggregator routers like 1inch and 0x can find the splitting that minimises total slippage across dozens of pools simultaneously.

### 9.2.5 LP economics: IL, fees, and the LVR decomposition

Holding an LP share in a constant-product pool is equivalent to:

- buying the pair at the time of deposit, and continuously
- selling the asset that goes up, buying the asset that goes down, along a deterministic schedule given by the AMM invariant.

The PnL of an LP relative to holding the two assets is the *impermanent loss* (IL):

$$
\boxed{\text{IL}(p) = \frac{2\sqrt{p}}{1 + p} - 1}, \quad p = \frac{p_t}{p_0}.
$$

The IL is always $\le 0$. Some reference values: $p = 1.25 \Rightarrow$ IL $\approx -0.62\%$; $p = 1.5 \Rightarrow$ $-2.02\%$; $p = 2 \Rightarrow$ $-5.72\%$; $p = 4 \Rightarrow$ $-20.0\%$.

This is implemented in `code/src/risk.rs` as `impermanent_loss`. Test `il_at_par_is_zero` and `il_2x_close_to_neg_5_72_pct` pin the values.

The flipside is *fees*: every swap pays $f \cdot |\Delta x|$ to LPs. Over a price path $p_t$, the cumulative fees received are roughly

$$
F_T \approx f \cdot \int_0^T |dV_t|
$$

where $V_t$ is dollar trading volume.

**LVR (Loss-Versus-Rebalancing).** A 2022 result by Milionis, Moallemi, Roughgarden, and Zhang shows that, against an arbitrageur with continuous access to a perfect external price, the per-unit-time loss of a CPMM LP is

$$
\text{LVR}(t) = \frac{\sigma^2}{8} \cdot V(t)
$$

where $\sigma$ is the volatility of the external price and $V(t)$ is the pool's mark-to-market value. This is the rebalancing leg of IL — the part the LP eats *every time* the external price moves, regardless of where it ends up. The remaining IL is "directional" and only realised on net price moves. The decomposition

$$
\text{IL} = \text{LVR} - \text{Drift}
$$

is the cleanest way to think about LP returns, because LVR is a continuous-time loss that volume cannot offset — only the gross fee revenue can. A passive LP is therefore profitable iff

$$
f \cdot \mathbb{E}[|dV_t|] > \frac{\sigma^2}{8}.
$$

This inequality is the heart of "is liquidity provisioning a good idea on this pair?" — and it answers in closed form once you have a volatility estimate.

### 9.2.6 Worked example: pricing a swap

Consider a Uniswap V2 ETH/USDC pool with 1,000 ETH and 2,000,000 USDC ($p_0 = 2{,}000$ USDC/ETH). Alice swaps 50 ETH. With $f = 0.003$:

$$
\Delta y = \frac{2{,}000{,}000 \cdot 0.997 \cdot 50}{1000 + 0.997 \cdot 50} = \frac{99{,}700{,}000}{1049.85} \approx 94{,}967\ \text{USDC}
$$

Effective price: $94{,}967 / 50 = 1{,}899.3$ USDC/ETH, i.e. **5.0% below the mid-price** of 2,000.

After the trade, reserves are $(1050, 1{,}905{,}033)$ and the new spot price is $1815.3$. A subsequent infinitesimal trade sees this price; an arbitrageur with access to a CEX where ETH still trades at 2,000 can buy ETH on Uniswap at $1815.3$ and sell on the CEX, capturing the gap. This is the setup for §9.3.

---

## 9.3 MEV — Maximal Extractable Value

### 9.3.1 Definition and history

**Definition.** *Maximal Extractable Value* (MEV) is the upper bound on the value that a block proposer (or, with private orderflow, a transaction-orderer one step removed) can extract from a sequence of pending transactions by reordering, inserting, or censoring them — over and above the standard block reward and direct transaction fees.

The term was introduced by Flashbots in their 2020 "MEV-Inspect" paper. Earlier work by Daian, Goldfeder et al. (2019, "Flash Boys 2.0") documented the same phenomenon under the name "frontrunner-as-a-service." By 2024, cumulative MEV extracted on Ethereum is estimated at well over $1.5 B.

MEV decomposes into three categories that we treat separately:

1. **Arbitrage MEV** — closing price discrepancies between pools or chains. Win-win economically; the only loser is the LP whose pool was mispriced.
2. **Sandwich (front-running) MEV** — extracting value from a victim user's swap by surrounding it. Pure value transfer; the victim is the loser.
3. **Liquidation MEV** — repaying a sub-collateralised CDP and seizing its collateral at a discount. Legitimate function of any over-collateralised lending protocol, but the discount is paid by the liquidatee.

Plus a long tail (JIT-LP, NFT-mint sniping, oracle MEV, governance attacks). The first three account for >95% of dollar volume.

### 9.3.2 Arbitrage MEV: closed-form for two pools

Setup: two CPMM pools $A$ and $B$ quote the same pair $(X, Y)$ at marginal prices $p_A = y_A/x_A$ and $p_B = y_B/x_B$, with $p_A < p_B$. The arbitrageur:

1. Borrows or holds $\Delta x$ of $X$.
2. Sells $\Delta x$ into pool $A$ (cheap pool), receiving $\Delta y_A = (1-f) y_A \Delta x / (x_A + (1-f)\Delta x)$.
3. Sells $\Delta y_A$ into pool $B$, receiving $\Delta x' = (1-f) x_B \Delta y_A / (y_B + (1-f)\Delta y_A)$.
4. Profit (in $X$ units): $\pi(\Delta x) = \Delta x' - \Delta x$.

We want $\Delta x^*$ that maximises $\pi$. The composition of two concave swap functions is concave (both have positive but decreasing derivative); $\pi$ is therefore concave on $[0, \infty)$ and has a unique interior maximum. Setting $\pi'(\Delta x) = 0$ and solving algebraically gives, after some manipulation,

$$
\boxed{\Delta x^* = \frac{\sqrt{x_A y_A x_B y_B \gamma^2} - x_A y_B}{x_A \gamma + y_B}, \quad \gamma = (1-f)^2.}
$$

(Other equivalent forms exist; this is the most numerically stable one for $\gamma$ close to 1.) The profit is positive iff the *fee-adjusted* prices are misaligned:

$$
\frac{y_A}{x_A} \cdot \gamma > \frac{y_B}{x_B}.
$$

This condition is the *no-arbitrage band*. Inside the band, fees swamp the gap and no profitable trade exists. The wider the band — i.e. the higher the swap fee — the more two pools can drift apart before arbitrageurs close the gap. This is one reason fee-tier choice on V3 matters: high-fee tiers (1% on exotic pairs) admit wide arb bands.

**Code reference.** `code/src/arbitrage.rs::two_pool_optimal` implements the closed form with a numerical fallback for edge cases (asymmetric fees, fee-on-transfer tokens). The unit test `arbitrage_profit_positive_when_prices_diverge` verifies positive EV when prices diverge by ~10%; `arbitrage_zero_when_prices_equal` verifies the no-arb condition.

### 9.3.3 Arbitrage MEV: triangular cycles

A more interesting case is a triangular cycle across three pools $(X, Y), (Y, Z), (Z, X)$. Starting from $\Delta x_0$ of $X$, after three hops we obtain

$$
\Delta x_3 = g_{Z \to X}(g_{Y \to Z}(g_{X \to Y}(\Delta x_0)))
$$

where each $g$ is the V2 swap function. The composition is still concave (composition of concave-and-increasing functions is concave), so a unique optimum exists. The closed form is messier — `cycle profit = 0` is a quartic in $\Delta x_0$ — and in practice one solves it numerically.

A useful diagnostic is the *cycle price product*:

$$
\rho = p_{X \to Y} \cdot p_{Y \to Z} \cdot p_{Z \to X} \cdot (1 - f)^3.
$$

If $\rho > 1$, the cycle is profitable; if $\rho \le 1$, no positive-EV trade exists. The exact optimum is then found by golden-section search on $[0, x_X / 2]$.

`code/src/arbitrage.rs::TriangularArb` implements both the simulator (`cycle_profit`) and the optimiser (`optimal_input`). Test `triangular_arb_returns_zero_for_consistent_prices` verifies that fee-only cycles never have positive EV.

### 9.3.4 Sandwich MEV: the searcher's perspective

A *sandwich* exploits a public mempool swap by surrounding it with two of the searcher's own:

- $T_{\text{front}}$: searcher swaps $\Delta x_f$ of $X$ into pool, pushing $p$ down.
- $T_{\text{victim}}$: victim swaps $\Delta x_v$ of $X$ into pool, executing at the worse post-$T_{\text{front}}$ price.
- $T_{\text{back}}$: searcher unwinds — swaps $\Delta y_f$ of $Y$ back, profiting from the gap.

The searcher's gross PnL (before gas) is

$$
\pi_{\text{search}}(\Delta x_f) = g_{Y \to X}\bigl(g_{X \to Y}(\Delta x_f)\bigr) - \Delta x_f.
$$

But $g_{X \to Y}$ here is taken *after* the victim's swap has executed against the front-run state. The exact computation requires simulating the three swaps in sequence — which is what `code/src/sandwich.rs::SandwichSimulator::simulate` does.

**The slippage-tolerance constraint.** A naïve sandwich would push the victim's execution beyond their slippage tolerance, causing $T_{\text{victim}}$ to revert. Since the back-leg depends on $T_{\text{victim}}$ executing successfully, this is a hard constraint: the searcher chooses $\Delta x_f$ to maximise profit *subject to the victim still receiving at least $\Delta y_v^{\min}$ from the pool*.

Formally:

$$
\Delta x_f^* = \arg\max_{\Delta x_f \ge 0} \; \pi_{\text{search}}(\Delta x_f) \quad \text{s.t.} \quad \tilde g_{X \to Y}(\Delta x_v \mid \text{after } T_{\text{front}}) \ge \Delta y_v^{\min}.
$$

The unconstrained optimum is interior and concave; the constraint may bind, in which case $\Delta x_f^*$ is exactly the largest front-run that leaves the victim at their slippage floor. `SandwichSimulator::optimal` uses golden-section search over the feasible region; if the entire region is infeasible (e.g. very tight tolerance), it returns `reverted = true`.

**Quant intuition.** The victim's slippage tolerance is the *width of the sandwich budget*. If a retail UI defaults to 0.5%, every retail swap is sandwich-able by approximately that much. This is why every wallet should default to *auto-slippage* — calculated from real-time pool depth — rather than a fixed percentage. The MEV tax on a 0.5%-tolerance pool transaction is bounded above by 0.5% per leg, hence the "0.5% slippage = 0.5% donation" rule of thumb. Tools like CowSwap and 1inch Fusion deflect this by using off-chain settlement and Dutch auctions; they're worth their own subsection (§9.4.4).

**Worked example.** Pool 1,000 ETH / 2,000,000 USDC, victim swaps 50 ETH with 2% slippage, no gas. Running `SandwichSimulator { gas_cost_x: 0.0 }.optimal(&pool, victim)`:

- Front-run: 23.4 ETH → 45,750 USDC
- Victim executes at the degraded price, receives ~92,200 USDC (vs ~94,970 unsandwiched, a loss of ~2,770 USDC = 2.9% of their order)
- Back-run: 45,750 USDC → 23.8 ETH (back to slightly more ETH than the front-run started with)
- **Searcher profit: ~0.4 ETH ≈ 800 USDC.**

The victim's loss exceeds the searcher's profit, by exactly the LP fees collected on both legs. Net effect: $f \cdot 2 \cdot \Delta x_f$ leaks to LPs.

### 9.3.5 Searcher economics: gas, competition, and the Kelly bound

A searcher who finds a positive-EV opportunity faces a sealed-bid first-price auction for inclusion: every competing searcher submits a bundle with a *priority fee* (tip to the validator), and the highest bid wins. By a standard auction-theoretic argument (Chapter 7), the equilibrium bid in a symmetric setting with $n$ identical bidders is

$$
b^* = \frac{n-1}{n} \cdot \pi_{\text{gross}}
$$

i.e. searchers compete away $(n-1)/n$ of the gross MEV to validators. With $n = 4$ active searchers on a given opportunity, ~75% of the MEV becomes validator revenue.

The breakeven priority fee is the maximum tip the searcher can afford and still net zero:

$$
b_{\text{breakeven}} = \frac{\pi_{\text{gross}}}{\text{gas\_used}} - \text{base\_fee}.
$$

`code/src/fees.rs::EffectiveFee::breakeven_priority_fee` computes this directly.

**Sizing.** Given uncertain success probability $p$ (your bid wins) and bankroll constraints, a Kelly-style sizing rule (Chapter 6) says: bid proportionally to your edge. Concretely, if you estimate gross profit $\pi$ and win probability $p$, the bid that maximises long-run wealth growth is

$$
b_{\text{Kelly}} = \pi \cdot \left(p - \frac{1-p}{R}\right)
$$

where $R$ is the gross-to-net ratio. For typical $R \approx 4$ and $p \approx 0.5$ (you win half the auctions you bid in), $b_{\text{Kelly}} \approx 0.375 \cdot \pi$ — leaving ample room above the breakeven floor.

### 9.3.6 Defensive MEV: protecting users

From a wallet/aggregator perspective, MEV protection has three pillars:

1. **Private mempools.** Submit transactions to a relay (Flashbots Protect, MEV-Blocker, Eden) that does not gossip them publicly. The relay forwards directly to block builders, who can include them without exposing them to the mempool.
2. **Auctioned settlement.** Protocols like CowSwap, 1inch Fusion, and UniswapX use a Dutch auction over off-chain solvers. Solvers compete to fill orders at the best price; only the winning fill is broadcast. This converts MEV from an extractive game (sandwich) into a productive one (best-execution competition).
3. **Auto-slippage.** Compute slippage tolerance dynamically from current pool depth and short-term volatility. Set it to just enough to absorb expected legitimate price drift between transaction send and inclusion; no more.

For a trading firm, these are not optional. A market-neutral strategy that pays 10 bps sandwich tax on every leg of every trade is leaking 100 bps a month in execution alpha.

---

## 9.4 Flash Loans and Composability

### 9.4.1 What a flash loan is

A *flash loan* is an atomic-borrow-and-repay primitive offered by lending protocols (Aave, Maker, Balancer, dYdX). The protocol lends an arbitrary amount $L$ to a caller; control returns to the caller's code; if at the end of the transaction the protocol does not hold $L \cdot (1+f)$ back in its reserves, the *entire transaction reverts*. The caller's risk is therefore exactly the gas cost — never the principal.

Three properties make this primitive transformative for quant strategies:

1. **No collateral.** Any strategy with positive expected gross PnL > $L \cdot f$ + gas is executable without capital.
2. **Atomicity.** The borrow, the strategy, and the repayment are one transaction. There is no rollover risk, no liquidation risk during execution, no settlement delay.
3. **Composability.** Inside the callback, the strategy can call *any* other smart contract — swap, lend, redeem, mint, burn. The only constraint is solvency at the end.

`code/src/flash_loan.rs` models this as a pure function: `FlashLoanPlan::net_profit(terminal_balance)` returns either the net profit (after fee and gas) or an `Insolvent` error.

### 9.4.2 Strategy templates

| Strategy | Flash-loan use |
|---|---|
| Multi-hop arb | Borrow $L$ of $X$, run the cycle, repay. Capital-free arb. |
| Self-liquidation | When near liquidation, borrow $L$ to repay debt, withdraw collateral, swap to debt asset, repay flash loan. Closes the position without an outsider liquidator's penalty. |
| Position migration | Move from Compound to Aave: flash-borrow debt, repay Compound, withdraw collateral, supply to Aave, borrow new debt, repay flash. |
| Collateral swap | Swap underlying asset of a leveraged position in one transaction. |
| Oracle attack | Use the flash loan to manipulate a low-liquidity AMM that an unrelated protocol uses as a price oracle. The cause of many DeFi exploits. |

The last entry deserves its own treatment.

### 9.4.3 Oracle attacks: the dark side of composability

A protocol that consumes a *spot price from an AMM* as its oracle is vulnerable. Within a single transaction, an attacker can:

1. Flash-loan $L$ of asset $X$.
2. Dump it all into a thin AMM pool, pushing the spot price to an extreme.
3. Call a target protocol that reads this manipulated price (e.g. to value collateral or to mint a synthetic).
4. Profit from the price manipulation (e.g. borrow far more than is safe).
5. Reverse the AMM trade, pay back the flash loan.

The target protocol's accounting now contains an asset valued at the manipulated price — by the time the next block arrives, the attacker has already left with the difference. Most large DeFi exploits of 2020–2023 follow this pattern: bZx ($1M, Feb 2020), Harvest ($24M, Oct 2020), Cream ($130M, Oct 2021), Mango ($117M, Oct 2022), Euler ($197M, Mar 2023).

The defence is to *never read a spot AMM price as an oracle for high-leverage decisions*. Use TWAPs over hundreds of blocks, dedicated oracle networks (Chainlink, Pyth, RedStone), or median-of-many pools weighted by liquidity.

### 9.4.4 Composable MEV protection (CowSwap, UniswapX)

A modern execution venue exploits flash-loan-like composability *defensively*. The flow is:

1. User signs an off-chain order: "I want to swap 10 ETH for at least 19,000 USDC."
2. The order enters a *batch*. Multiple users' orders are pooled.
3. A network of *solvers* competes to find an execution plan that satisfies all orders (using DEXes, peer-to-peer matching inside the batch, market-maker quotes, etc.).
4. The winning solver atomically executes the plan on-chain in a single transaction.

Because the order is never in the public mempool, sandwich attacks are impossible: by the time the transaction lands, the trade is already settled. Because solvers compete, the user receives the best price available across the entire ecosystem — often *better* than direct DEX execution. CowSwap routinely reports negative slippage (price improvement) on the order of 5–15 bps.

The mechanism design is closer to a sealed-bid second-price auction (Chapter 7) than to a first-price gas auction. The result is dramatically lower extraction.

---

## 9.5 On-Chain Risk Management

### 9.5.1 Position dimensions

A DeFi position has risks not present in CEX trading. We catalogue them:

| Dimension | Magnitude (typical) | Mitigation |
|---|---|---|
| Market risk | Same as CEX | Hedge, size by Kelly |
| Protocol risk | 1–5% / year per protocol | Diversify protocols, age-weight (older = safer) |
| Oracle risk | Tail event, 100% loss | Avoid spot-AMM-oracle exposures |
| Bridge risk | Tail event, partial loss | Use canonical bridges only; minimise time-in-bridge |
| Liquidation risk | Programmatic, depends on HF | Maintain HF > 1.5; monitor with off-chain alerts |
| Validator/MEV risk | 5–15 bps per trade | Use private mempool / batch auctions |
| Gas risk | Variable; spikes 10–100× on chain congestion | Set gas ceilings; have fallback to L2 |
| Smart-contract upgrade risk | Sudden | Read governance proposals; have exit plan |

### 9.5.2 Liquidation distance and health factor

For an over-collateralised CDP with collateral $C$ priced at $p$, max LTV $\ell$, and debt $D$, the *health factor* is

$$
\text{HF} = \frac{C \cdot p \cdot \ell}{D}.
$$

When HF drops below 1, the position becomes liquidatable: anyone may repay some fraction of $D$ in exchange for the equivalent collateral plus a *liquidation bonus* (typically 5–15%). The collateral price at which HF = 1 is

$$
p_{\text{liq}} = \frac{D}{C \cdot \ell}.
$$

The *liquidation distance*, expressed as a fraction below current price, is $(p - p_{\text{liq}}) / p$. A position with HF = 1.5 and $\ell = 0.8$ has $p_{\text{liq}} = p / 1.5 \cdot 0.8 / 0.8 = p / 1.5$, i.e. **a 33% drop wipes it**.

`code/src/risk.rs::health_factor` and `liquidation_distance` compute these. The test `liquidation_when_collateral_drops` walks through a 10-ETH, $10k-debt example.

**Stochastic liquidation probability.** Treating $p_t$ as a GBM with drift $\mu$ and volatility $\sigma$ (Chapter 1), the probability of liquidation within horizon $T$ is (first-passage of GBM):

$$
\mathbb{P}(\tau \le T) = \Phi\left(\frac{-\ln(p_0 / p_{\text{liq}}) + \mu T}{\sigma \sqrt{T}}\right) + \left(\frac{p_{\text{liq}}}{p_0}\right)^{2\mu/\sigma^2} \Phi\left(\frac{-\ln(p_0 / p_{\text{liq}}) - \mu T}{\sigma \sqrt{T}}\right)
$$

where $\Phi$ is the standard normal CDF. For ETH at HF = 1.5 with $\sigma = 0.8$ annualised, the 30-day liquidation probability is around 5%. This is the number that should drive position sizing on a leveraged collateral position — not a static HF threshold.

### 9.5.3 Putting it together: the on-chain Kelly bound

Chapter 6 gives the Kelly-optimal fraction for a bet with edge $e$ and odds $b$:

$$
f^* = \frac{e}{b}.
$$

On-chain, the *effective edge* is the gross alpha *minus* MEV tax *minus* protocol-risk-adjusted expected loss. The *effective odds* should incorporate liquidation tails. For a strategy with 50 bps gross alpha, 10 bps MEV tax, 2% annual protocol-risk fail-rate, and a position sized to keep liquidation probability $< 1\%$, the Kelly fraction shrinks substantially relative to a naïve CEX calculation. A useful rule of thumb:

$$
f^*_{\text{on-chain}} \approx 0.5 \cdot f^*_{\text{CEX-equivalent}}.
$$

This is conservative and accommodates the fact that protocol-risk and oracle-risk are heavy-tailed.

### 9.5.4 Operational checklist

Before deploying a strategy on-chain, the quant should be able to fill out the following:

- [ ] Mark-to-market function written down for every position type.
- [ ] Liquidation thresholds computed and monitored, with off-chain alerts.
- [ ] Slippage tolerance is auto-computed, not a fixed percentage.
- [ ] Transactions sent through a private mempool or batch auction.
- [ ] Oracle dependencies enumerated and stress-tested under flash-loan manipulation.
- [ ] Gas-price ceiling defined; strategy reverts (not stuck) above the ceiling.
- [ ] Smart-contract upgrade plans for every dependency monitored.
- [ ] Bridge exposure minimised; positions held on the chain they trade on.
- [ ] Protocol concentration limit (e.g. no more than 25% of capital in any single protocol).
- [ ] Backtest includes protocol fees, price impact, gas, and an MEV tax estimate.

The last point is worth repeating: a backtest that omits the MEV tax can overstate strategy edge by 5–20 bps per trade — easily the difference between a profitable and a money-losing strategy in market-neutral arbitrage.

---

## 9.6 Conclusion

DeFi and MEV change the substrate of trading, not its goal. The goal — find positive-EV deployments of capital subject to a risk budget — is unchanged from Chapter 1. The substrate — closed-form swap functions instead of order books, sealed-bid block auctions instead of FIFO matching, atomic-compositional execution instead of sequential settlement — produces new closed-form formulas, new risk dimensions, and new strategy archetypes.

The most important takeaway, in our experience, is that **the math is more tractable on-chain than off-chain**. The CPMM swap function is a single equation. Optimal arbitrage between two pools is a single closed-form expression. Sandwich PnL is a single composition of two swap functions. Liquidation distance is a one-line formula. There is no order-flow noise, no hidden depth, no broker rebates, no Reg NMS routing. The trade-off is that these closed forms operate in an adversarial environment where every action is observable in the mempool — so winning is not about edge in the model, but speed and stealth in execution.

We covered:

- §9.1: How a blockchain looks to a market-maker, the DEX landscape, and what it costs to trade on-chain.
- §9.2: Constant-product, concentrated-liquidity, and StableSwap AMMs, with closed-form pricing, IL, and LVR.
- §9.3: Arbitrage MEV with closed-form optimums, sandwich MEV with slippage constraints, and the gas auction that determines extraction.
- §9.4: Flash loans as a composability primitive — capital-free strategies, oracle attacks, batch auctions as defence.
- §9.5: On-chain risk decomposition, liquidation probability, and an on-chain Kelly bound.

The companion Rust crate (`code/`) provides reference implementations for every formula in this chapter, complete with unit tests and Criterion benchmarks. Read the tests as worked examples; run the benchmarks to feel the performance budget; modify the strategies to evaluate your own ideas.

### Forward references

- **Chapter 10 (Cross-chain arbitrage)** generalises §9.3 to multi-chain bridges and the latency/atomicity trade-off they introduce.
- **Chapter 11 (Quant infrastructure)** specifies the off-chain architecture — mempool ingestion, simulation, bundle submission — needed to run §9.3 in production.
- **Chapter 12 (Strategy backtesting)** revisits §9.5's operational checklist as concrete test cases against historical on-chain data.

### Exercises

1. Derive the closed-form optimal arbitrage for two CPMM pools with *different* fees $f_A, f_B$. (Hint: the symmetry breaks; you get a different quadratic.)
2. Show analytically that LVR for a CPMM LP is exactly $\sigma^2 / 8$ per unit time when the external price follows GBM with volatility $\sigma$. (Hint: marginal-price impact times rebalance volume.)
3. Implement a V3 swap that traverses *multiple* ticks. The crate stops at one; extend the loop. Verify it matches the on-chain output of a real Uniswap V3 pool.
4. Estimate the MEV tax on a representative ETH/USDC trade by sampling 100 historical sandwiches from Flashbots' public dashboard. Compare to the back-of-envelope formula from §9.3.4.
5. Build a real-time monitor for the health factor of a leveraged ETH position on Aave. Trigger an alert when the 30-day liquidation probability (formula §9.5.2) exceeds 5%.
6. Add a JIT-LP strategy to the simulator: deposit a V3 position in a single block before a known large swap, capture the fees, withdraw. Estimate how much edge JIT-LP siphons from passive LPs in a representative day.

### Further reading

- Daian, P. et al. "Flash Boys 2.0: Frontrunning in Decentralized Exchanges, Miner Extractable Value, and Consensus Instability" (2019).
- Adams, H., Zinsmeister, N., Salem, M., Keefer, R., Robinson, D. "Uniswap V3 Core" whitepaper (2021).
- Egorov, M. "StableSwap — efficient mechanism for Stablecoin liquidity" (2019).
- Milionis, J., Moallemi, C. C., Roughgarden, T., Zhang, A. L. "Quantifying Loss in Automated Market Makers" (2022) — the LVR paper.
- Qin, K., Zhou, L., Gervais, A. "Quantifying Blockchain Extractable Value: How dark is the forest?" (2022).
- Flashbots research blog: [writings.flashbots.net](https://writings.flashbots.net).
