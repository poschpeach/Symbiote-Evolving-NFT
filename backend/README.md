# Symbiote Backend

Express + Solana WebSocket listener + OpenAI inference + Jupiter swap builder.

## Endpoints

- `POST /auth/challenge`
  - body: `{ "walletAddress": "..." }`
  - returns message+nonce for Phantom `signMessage`
- `POST /auth/verify`
  - body: `{ "walletAddress": "...", "signatureBase64": "..." }`
  - returns bearer token
- `POST /connect-wallet`
  - requires bearer auth
  - body: `{ "walletAddress": "...", "symbioteMint": "..." }`
- `POST /mint-symbiote`
  - requires bearer auth
  - body: `{ "walletAddress": "..." }`
  - mints mutable NFT via Anchor `mint_symbiote` and stores mint for that wallet
- `POST /suggest-trade`
  - requires bearer auth
  - body: `{ "walletAddress": "..." }`
  - returns AI profile, personality reaction, and Jupiter `readyToSignSwapTransaction`
- `POST /confirm-trade`
  - requires bearer auth
  - body: `{ "walletAddress": "...", "signature": "..." }`
  - verifies Jupiter transaction and calls on-chain `evolve_symbiote`
- `GET /symbiote/:mint`
  - requires bearer auth
  - returns live on-chain symbiote state
- `GET /metadata/:mint/state.json`
  - public metadata JSON endpoint
- `GET /sample-jupiter-transaction`
  - returns static sample routed swap object

## Setup

1. `cp .env.example .env`
2. Set required keys.
3. `npm install`
4. `npm run validate:env`
5. `npm run start`

Optional smoke test (backend must be running):

```bash
TEST_WALLET_ADDRESS=YourWalletPubkey npm run smoke
```

Backend now enables CORS for browser frontend testing.

## Notes

- `JUPITER_REFERRAL_FEE_ACCOUNT` must be your valid referral fee token account.
- backend signer (evolution authority):
  - set `SYMBIOTE_KEYPAIR_BASE58`, or
  - set `SYMBIOTE_KEYPAIR_FILE` (defaults to `~/.config/solana/id.json`)
- Default IDL path is bundled: `./idl/symbiote_pet.json`.
- For strict parity after rebuild/redeploy, point `SYMBIOTE_IDL_PATH` to freshly generated Anchor IDL.
- `POST /confirm-trade` is guarded by:
  - signer-wallet match
  - replay prevention (unique tx signature)
  - minimum volume threshold (`MIN_CONFIRM_VOLUME_USD`)
