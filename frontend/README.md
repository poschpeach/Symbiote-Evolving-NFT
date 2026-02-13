# Symbiote Frontend (Minimal)

## Run

1. Start backend:

```bash
cd /Users/defiduke/Documents/New\ project/backend
npm install
npm run start
```

2. Serve frontend as static files:

```bash
cd /Users/defiduke/Documents/New\ project
python3 -m http.server 5173 --directory frontend
```

3. Open:

- `http://localhost:5173`

Or run both backend + frontend from workspace root:

```bash
cd /Users/defiduke/Documents/New\ project
./scripts/dev-up.sh
```

## Flow

1. Connect Phantom (includes auth challenge + signed verification)
2. Connect wallet session to backend
3. Mint Symbiote NFT
4. Request AI trade suggestion + Jupiter routed transaction
5. Sign and send transaction in Phantom
6. Backend confirms trade and calls `evolve_symbiote`
7. Frontend auto-refreshes `/symbiote/:mint` every 10s
