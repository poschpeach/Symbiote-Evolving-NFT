#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BACKEND_DIR="$ROOT_DIR/backend"
FRONTEND_DIR="$ROOT_DIR/frontend"
LOG_DIR="$ROOT_DIR/.logs"
mkdir -p "$LOG_DIR"

BACKEND_PORT="${BACKEND_PORT:-}"
if [[ -z "$BACKEND_PORT" && -f "$BACKEND_DIR/.env" ]]; then
  BACKEND_PORT="$(grep -E '^PORT=' "$BACKEND_DIR/.env" | tail -n1 | cut -d'=' -f2)"
fi
BACKEND_PORT="${BACKEND_PORT:-3000}"

cleanup() {
  if [[ -n "${BACKEND_PID:-}" ]]; then kill "$BACKEND_PID" 2>/dev/null || true; fi
  if [[ -n "${FRONTEND_PID:-}" ]]; then kill "$FRONTEND_PID" 2>/dev/null || true; fi
}
trap cleanup EXIT

cd "$BACKEND_DIR"
npm install
npm run validate:env
PORT="$BACKEND_PORT" npm run start >"$LOG_DIR/backend.log" 2>&1 &
BACKEND_PID=$!

sleep 2
curl -fsS "http://127.0.0.1:${BACKEND_PORT}/health" >/dev/null

cd "$ROOT_DIR"
python3 -m http.server 5173 --directory "$FRONTEND_DIR" >"$LOG_DIR/frontend.log" 2>&1 &
FRONTEND_PID=$!

echo "Backend:  http://127.0.0.1:${BACKEND_PORT}"
echo "Frontend: http://127.0.0.1:5173"
echo "Logs: $LOG_DIR"
echo "Press Ctrl+C to stop."

wait
