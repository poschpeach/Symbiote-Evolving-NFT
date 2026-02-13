# Files and Descriptions

## Root

- `README.md`: top-level project overview and quick start.
- `.gitignore`: excludes secrets, DB, logs, and node modules.
- `scripts/dev-up.sh`: starts backend and frontend together.
- `scripts/preflight.sh`: environment/tooling readiness checks.

## Backend (`backend/`)

- `server.js`: main API server and business logic.
  - auth challenge/session
  - wallet connect/mint/suggest/confirm
  - agentic game loop endpoints and auto-play scheduler
  - Jupiter quote/swap construction
  - on-chain evolve call
  - state + metadata endpoints
- `.env.example`: required configuration template.
- `idl/symbiote_pet.json`: fallback IDL used by backend client.
- `sample-jupiter-transaction.json`: sample Jupiter routed tx payload.
- `README.md`: backend endpoint and setup docs.
- `package.json`: runtime deps and npm scripts.
- `scripts/validate-env.mjs`: strict `.env` validator.
- `scripts/smoke-test.mjs`: quick API smoke test.

## Frontend (`frontend/`)

- `index.html`: minimal UI shell.
- `styles.css`: styling and layout.
- `app.js`: Phantom + backend orchestration logic.
- `README.md`: frontend run instructions.

## On-chain Program (`symbiote-anchor/`)

- `programs/symbiote_pet/src/lib.rs`: Anchor program logic.
- `programs/symbiote_pet/Cargo.toml`: program crate deps.
- `Anchor.toml`: Anchor workspace config.
- `Cargo.toml`: anchor workspace cargo config.
- `tests/symbiote_pet.ts`: integration test for mint/evolve.
- `scripts/deploy-dev.sh`: build/deploy helper.
- `README.md`: program build/test/deploy notes.
- `package.json`, `tsconfig.json`: JS/TS test toolchain.

## Docs (`docs/`)

- `HACKATHON_READINESS.md`: readiness status and remaining blockers.
- `HACKATHON_RUNBOOK.md`: live demo runbook and fallback plan.
- `FILES_AND_DESCRIPTIONS.md`: this file.
