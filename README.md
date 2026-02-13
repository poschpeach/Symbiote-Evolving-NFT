# Aegis Protocol MVP (Rust)

Autonomous risk-management agent for Solana-style DeFi positions.

This build now supports two modes:
- `scripted`: deterministic demo feed
- `live`: real Helius + Pyth/Jupiter market reads

## What It Does

- monitors a leveraged position health factor
- runs deterministic risk policy (`hold` or `partial_unwind`)
- computes dynamic priority fee based on live fee pressure
- uses live Jupiter quote data for unwind proceeds estimation
- appends an auditable "proof-of-thought" record to CSV
- serves local dashboard JSON for demos

Current execution is still simulated (safe by default): no signed transaction is sent.

## Run (Scripted Demo)

```bash
cargo run
```

## Run (Live Data)

```bash
export AEGIS_MODE=live
export AEGIS_HELIUS_RPC_URL='https://mainnet.helius-rpc.com/?api-key=YOUR_KEY'
cargo run
```

Optional tuning:

```bash
export AEGIS_MAX_CYCLES=30
export AEGIS_POLL_MS=1200
export AEGIS_LIVE_QUOTE_EXEC=true
export AEGIS_DANGER_HF=1.08
export AEGIS_TARGET_HF=1.25
cargo run
```

## Dashboard

JSON endpoint:
- `http://127.0.0.1:8080`

Disable dashboard:

```bash
export AEGIS_DASHBOARD=false
```

## Key Environment Variables

Mode/runtime:
- `AEGIS_MODE` = `scripted` or `live` (default `scripted`)
- `AEGIS_POLL_MS` (default `700`)
- `AEGIS_MAX_CYCLES` (default `20`)
- `AEGIS_AUDIT_LOG` (default `aegis_actions.csv`)

Risk:
- `AEGIS_DANGER_HF` (default `1.08`)
- `AEGIS_TARGET_HF` (default `1.25`)
- `AEGIS_EMERGENCY_HF` (default `1.02`)
- `AEGIS_MAX_UNWIND_PCT` (default `0.35`)
- `AEGIS_COOLDOWN_SLOTS` (default `2`)
- `AEGIS_MAX_SLIPPAGE_BPS` (default `60`)

Position:
- `AEGIS_WALLET` (default `demo-wallet`)
- `AEGIS_COLLATERAL_SOL` (default `18.0`)
- `AEGIS_STABLE_BALANCE` (default `300.0`)
- `AEGIS_DEBT_USDC` (default `3300.0`)

Live data/execution:
- `AEGIS_HELIUS_RPC_URL` (required in live mode)
- `AEGIS_PYTH_HERMES_URL` (default `https://hermes.pyth.network`)
- `AEGIS_PYTH_SOL_FEED_ID` (default SOL/USD feed id)
- `AEGIS_JUPITER_PRICE_URL` (default `https://lite-api.jup.ag/price/v3`)
- `AEGIS_JUPITER_QUOTE_URL` (default `https://lite-api.jup.ag`)
- `AEGIS_JUPITER_API_KEY` (optional)
- `AEGIS_LIVE_QUOTE_EXEC` (default `true`)

## Output

Audit trail CSV:
- `aegis_actions.csv`

Columns include:
- source (`scripted` or `live-helius-pyth`)
- price + health factor
- action + sold/repaid amounts
- quote source (`jupiter-ultra` or fallback)
- tx id + proof digest + reason

## Test

```bash
cargo test
```

## Next Steps (Production)

- Drift/Save adapters for real position read/write
- signed Jupiter swap-instructions execution with constrained vault authority
- full websocket stream adapters (Helius + Pyth)
- zk proof generation for policy rule attestation
