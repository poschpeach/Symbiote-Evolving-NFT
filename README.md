# Symbiote Pet Dynamic Evolutionary NFT

Autonomous Solana NFT game agent that:
- mints a mutable Metaplex NFT,
- learns wallet behavior from on-chain activity,
- plays agentic strategy turns for the user (manual + auto-play),
- plans multi-domain autonomous actions (portfolio, yield, governance, game missions),
- suggests Jupiter swaps with referral fee routing,
- and evolves NFT state (Level / XP / Personality) after executed trades.

## Project Structure

- `symbiote-anchor/`: on-chain program (Anchor/Rust)
- `backend/`: Express + OpenAI + Solana listeners + Jupiter integration
- `frontend/`: minimal Phantom web UI
- `scripts/`: local orchestration scripts
- `docs/`: hackathon runbook and readiness checklist

## Quick Start

1. Configure backend environment:

```bash
cd /Users/defiduke/Documents/New\ project/backend
cp .env.example .env
npm install
npm run validate:env
```

2. Launch backend + frontend:

```bash
cd /Users/defiduke/Documents/New\ project
./scripts/dev-up.sh
```

3. Open:
- `http://localhost:5173`

## Demo Flow

1. Connect Phantom (signed auth challenge)
2. Mint Symbiote NFT
3. Generate AI trade suggestion
4. Sign Jupiter swap transaction
5. Confirm trade and evolve NFT on-chain
6. Watch live state updates via `/symbiote/:mint`

## Readiness

See:
- `docs/HACKATHON_RUNBOOK.md`
- `docs/HACKATHON_READINESS.md`
