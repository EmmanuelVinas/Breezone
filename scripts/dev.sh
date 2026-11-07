#!/usr/bin/env bash
# Development without Docker: fake Airzone + Breezone server (cargo) + web UI (Vite, hot reload).
#
#   ./scripts/dev.sh            # http://localhost:5173 (Vite) → proxies /api to :8427
#   ./scripts/dev.sh --reset    # start from an empty database (data/dev)
#   ./scripts/dev.sh --real     # no fake Airzone: use BREEZONE_* from .env (real WebServers)
#   BREEZONE_DEV_PORT=8091 ./scripts/dev.sh   # server on another port (e.g. for the mobile E2E test)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="${SCRIPT_DIR}/.."
cd "${PROJECT_DIR}"

FAKE=true
RESET=false
for arg in "$@"; do
  case $arg in
    --reset) RESET=true ;;
    --real) FAKE=false ;;
    -h|--help) sed -n '2,7p' "$0"; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

if [ -f .env ]; then
  set -a
  # shellcheck source=../.env
  source .env
  set +a
fi

DATA_DIR="${PROJECT_DIR}/data/dev"
if [ "${RESET}" = "true" ]; then
  rm -rf "${DATA_DIR}"
  echo "▸ Database reset"
fi
mkdir -p "${DATA_DIR}"

PIDS=()
cleanup() { kill "${PIDS[@]}" 2>/dev/null || true; }
trap cleanup EXIT INT TERM

DEV_PORT="${BREEZONE_DEV_PORT:-8427}"
export BREEZONE_PORT="${DEV_PORT}" BREEZONE_DATA_DIR="${DATA_DIR}" BREEZONE_WEB_DIR="${PROJECT_DIR}/web/dist"
export BREEZONE_DEV_SERVER="http://localhost:${DEV_PORT}"
if [ "${FAKE}" = "true" ]; then
  echo "▸ Fake Airzone WebServer :3000 + fake Airzone Cloud :3900 (demo@breezone.local / demo-password)"
  (cd server && MOCK_PORT=3000 MOCK_CLOUD_PORT=3900 cargo run -q --bin mock-airzone) &
  PIDS+=($!)
  export BREEZONE_AIRZONE_CLOUD_URL="http://127.0.0.1:3900" BREEZONE_SCAN_SUBNET="127.0.0.0/30" BREEZONE_POLL_INTERVAL_SECS=5
fi

echo "▸ Breezone server :${DEV_PORT}"
(cd server && cargo run -q --bin breezone-server) &
PIDS+=($!)

until curl -sf "http://localhost:${DEV_PORT}/health" > /dev/null 2>&1; do sleep 1; done
echo "▸ Web UI (Vite) → http://localhost:5173"
(cd web && npm run dev -- --host) &
PIDS+=($!)
wait
