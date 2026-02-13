Symbiote is an autonomous agentic system and evolutionary NFT companion. It manages a complete agentic flow: learning user behavior, planning complex financial actions, and executing Jupiter trades. As a true on-chain companion, it autonomously evolves its Level, XP, and Personality based on outcome data. A living, thinking financial operator on Solana.

The system:
- mints and maintains a living NFT identity on Solana,
- learns a user profile from wallet behavior and historical actions,
- runs a persistent mission/game loop (manual turns + auto-play),
- plans cross-domain actions (trading, yield posture, risk recovery, governance-style objectives),
- prepares Jupiter-routed transactions with referral routing for execution,
- and evolves its on-chain state (`Level`, `XP`, `Personality`) after verified outcomes.

The result is a programmable “pet strategist” that behaves like a game character + financial operator, with state anchored on-chain.


## Project Structure

- `symbiote-anchor/`: on-chain program (Anchor/Rust)
- `backend/`: Express + OpenAI + Solana listeners + Jupiter integration
- `frontend/`: minimal Phantom web UI
- `scripts/`: local orchestration scripts
- `docs/`: hackathon runbook and readiness checklist

## Quick Start

1. Configure backend environment:

```bash
cd backend
cp .env.example .env
npm install
npm run validate:env
```

2. Launch backend + frontend:

```bash
# return to root if you are in /backend
cd ..
./scripts/dev-up.sh
```

3. Open:
- `http://localhost:5173`

## Demo Flow

1. Connect Phantom (signed auth challenge)
2. Mint Symbiote NFT
3. Create a mission and run agent turns (`/agent/create-mission`, `/agent/play-turn`)
4. Generate broader autonomous action plan (`/agent/next-actions`)
5. Sign Jupiter transaction when the agent proposes an executable move
6. Confirm outcome and evolve NFT on-chain (`/confirm-trade`)
7. Inspect unified live dashboard (`/agent/dashboard/:walletAddress`)

## Readiness

See:
- `docs/HACKATHON_RUNBOOK.md`
- `docs/HACKATHON_READINESS.md`
