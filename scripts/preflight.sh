#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BACKEND_ENV="$ROOT_DIR/backend/.env"
SOLANA_BIN="$HOME/.local/share/solana/install/active_release/bin"

if [[ -d "$SOLANA_BIN" ]]; then
  export PATH="$SOLANA_BIN:$PATH"
fi

echo "== Symbiote Preflight =="

if ! command -v node >/dev/null 2>&1; then
  echo "Missing: node"
  exit 1
fi

if ! command -v npm >/dev/null 2>&1; then
  echo "Missing: npm"
  exit 1
fi

if ! command -v anchor >/dev/null 2>&1; then
  echo "Missing: anchor CLI"
else
  anchor --version
fi

if ! command -v solana >/dev/null 2>&1; then
  echo "Missing: solana CLI"
else
  solana --version
fi

if [[ ! -f "$BACKEND_ENV" ]]; then
  echo "Missing backend/.env"
  exit 1
fi

cd "$ROOT_DIR/backend"
npm run validate:env

if grep -q "REPLACE_WITH_YOUR_REFERRAL_TOKEN_ACCOUNT" "$BACKEND_ENV"; then
  echo "Warning: JUPITER_REFERRAL_FEE_ACCOUNT still placeholder."
fi

echo "Preflight done."
