#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v anchor >/dev/null 2>&1; then
  echo "anchor CLI not found."
  exit 1
fi

echo "Building program..."
anchor build

echo "Deploying program..."
anchor deploy

IDL_SRC="$ROOT_DIR/target/idl/symbiote_pet.json"
if [[ ! -f "$IDL_SRC" ]]; then
  echo "IDL missing at $IDL_SRC"
  exit 1
fi

echo "Program deployed. IDL generated at:"
echo "  $IDL_SRC"
echo "Update backend .env SYMBIOTE_IDL_PATH if needed."
