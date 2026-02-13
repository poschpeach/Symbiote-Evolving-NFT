# Hackathon Readiness

## Current Status

- Backend: ready (auth, rate-limit, trade validation, replay protection, metadata endpoint)
- Frontend: ready (Phantom auth + swap sign flow + live state refresh)
- Anchor program code: ready
- End-to-end local smoke checks: passed

## Must-Fix Before Final Demo Submission

1. Install blockchain toolchain on your machine:
   - `anchor` CLI
   - `solana` CLI
2. Deploy actual on-chain program and verify:
   - `SYMBIOTE_PROGRAM_ID`
   - `SYMBIOTE_IDL_PATH` points to deployed build IDL
3. Set real referral account:
   - `JUPITER_REFERRAL_FEE_ACCOUNT` in `backend/.env`
4. Rotate and replace leaked OpenAI API key.
5. If external judges view metadata, use public metadata host (not localhost URI base in program), redeploy.

## Done Already

- Session auth via signed challenge
- Wallet/action authorization checks
- Jupiter transaction signer check
- Unique tx replay prevention
- Minimum volume threshold check
- SQLite persistence (memory, users, suggestions, trades)
- Public metadata JSON generation route
- One-command local startup script

## Go/No-Go

- Go for local/private demo: yes
- Go for public/judged demo: after completing "Must-Fix" list above
