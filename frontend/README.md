# Symbiote Frontend (Minimal)

## Run

1. Start backend:

```bash
# from project root
cd backend
npm install
npm run start
```

2. Serve frontend as static files:

```bash
# from project root
python3 -m http.server 5173 --directory frontend
```

3. Open:

- `http://localhost:5173`

Or run both backend + frontend from workspace root:

```bash
# from project root
./scripts/preflight.sh
./scripts/dev-up.sh
```

## Flow

1. Connect Phantom (includes auth challenge + signed verification)
2. Connect wallet session to backend
3. Mint Symbiote NFT
4. Play autonomous game turn (`/agent/play-turn`)
5. Optionally toggle auto-play (`/agent/auto-play`)
6. Request AI trade suggestion + Jupiter routed transaction
7. Sign and send transaction in Phantom
8. Backend confirms trade and calls `evolve_symbiote`
9. Frontend auto-refreshes `/symbiote/:mint` and `/agent/state/:wallet` every 10s
