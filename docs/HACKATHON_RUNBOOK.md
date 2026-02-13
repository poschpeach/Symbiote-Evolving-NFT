# Hackathon Runbook

## 1) What Each Part Does

### `symbiote-anchor/`

- Anchor program for NFT lifecycle.
- `mint_symbiote(owner)`:
  - creates mint,
  - mints 1 token,
  - creates mutable metadata + master edition,
  - initializes state account with default personality + URI.
- `evolve_symbiote(nft_account, new_stats)`:
  - updates state fields (`level`, `xp`, `personality`),
  - updates Metaplex metadata URI.

Key file:
- `symbiote-anchor/programs/symbiote_pet/src/lib.rs`

### `backend/`

- Wallet auth:
  - `/auth/challenge`
  - `/auth/verify`
- Wallet binding + listener:
  - `/connect-wallet`
- Agentic game loop:
  - `/agent/play-turn`
  - `/agent/state/:walletAddress`
  - `/agent/auto-play`
- Mint:
  - `/mint-symbiote`
- AI + Jupiter planning:
  - `/suggest-trade`
- Post-trade verify + evolve:
  - `/confirm-trade`
- Read state:
  - `/symbiote/:mint`
- Metadata serving:
  - `/metadata/:mint/state.json`

Key file:
- `backend/server.js`

### `frontend/`

- Minimal Phantom UI:
  - connect wallet,
  - sign auth challenge,
  - mint,
  - play agent game turn,
  - toggle agent auto-play,
  - suggest swap,
  - sign/send Jupiter tx,
  - confirm trade,
  - auto-refresh symbiote + game state.

Key file:
- `frontend/app.js`

## 2) Local Start

```bash
cd /Users/defiduke/Documents/New\ project
./scripts/dev-up.sh
```

Then open:
- `http://localhost:5173`

## 3) Live Demo Script

1. Connect Phantom.
2. Mint Symbiote.
3. Click Play Agent Turn and show narrative/action.
4. Click Suggest Trade (or use tx returned by game turn).
5. Sign Jupiter tx in Phantom.
6. Confirm evolution response in UI.
7. Show `level/xp/personality` and game profile changes in state panel.

## 4) API Sequence (Frontend)

1. `POST /auth/challenge`
2. Sign message in wallet
3. `POST /auth/verify`
4. `POST /connect-wallet` (bearer token)
5. `POST /mint-symbiote`
6. `POST /agent/play-turn`
7. `POST /suggest-trade`
8. Wallet signs tx
9. `POST /confirm-trade`
10. `GET /symbiote/:mint`
11. `GET /agent/state/:walletAddress`

## 5) Demo Safety Fallbacks

- If Jupiter quote fails:
  - retry `/suggest-trade`
  - lower size via recommendation amount logic
- If tx lands but confirm fails:
  - retry `/confirm-trade` with same signature (non-replay if first write failed)
- If RPC flaky:
  - restart backend
  - switch RPC endpoint in `.env`
