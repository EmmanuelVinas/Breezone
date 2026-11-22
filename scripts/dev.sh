#!/usr/bin/env bash
# Development without Docker: fake Airzone + Breezone server (cargo) + web UI (Vite, hot reload).
#
#   ./scripts/dev.sh            # http://localhost:5173 (Vite) → proxies /api to :8427
#   ./scripts/dev.sh --reset    # start from an empty database (data/dev)
#   ./scripts/dev.sh --real     # no fake Airzone: use BREEZONE_* from .env (real WebServers)
#   ./scripts/dev.sh --modules  # also one fake module of each kind (lights, shutters, energy…), added
#                               # for the admin (BREEZONE_ADMIN_USER / _PASSWORD, admin / motdepasse1)
#   ./scripts/dev.sh --shipped  # the modules shipped with Breezone (WebAssembly) in place of the drivers
#                               # compiled in, as in the image (built and packed into data/dev-modules)
#   BREEZONE_DEV_PORT=8091 ./scripts/dev.sh   # server on another port (e.g. for the mobile E2E test)
#   BREEZONE_DEV_DATA=data/screenshots ./scripts/dev.sh --reset --modules   # a fresh demonstration home

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="${SCRIPT_DIR}/.."
cd "${PROJECT_DIR}"

FAKE=true
RESET=false
MODULES=false
SHIPPED=false
for arg in "$@"; do
  case $arg in
    --reset) RESET=true ;;
    --real) FAKE=false ;;
    --modules) MODULES=true ;;
    --shipped) SHIPPED=true ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

if [ -f .env ]; then
  set -a
  # shellcheck source=../.env
  source .env
  set +a
fi

# BREEZONE_DEV_DATA: another database (screenshots on a fresh one, without your own data).
DATA_DIR="${BREEZONE_DEV_DATA:-${PROJECT_DIR}/data/dev}"
# Absolute: the server runs from server/.
case "${DATA_DIR}" in /*) ;; *) DATA_DIR="$(pwd)/${DATA_DIR}" ;; esac
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
  MOCK_DEVICES=""
  # With the modules, the weather too (Open-Meteo is asked for the city of the installation).
  [ "${MODULES}" = "true" ] && export BREEZONE_WEATHER="${BREEZONE_WEATHER:-1}" BREEZONE_FORECAST_SOLAR_URL="http://127.0.0.1:3106"
  [ "${MODULES}" = "true" ] && MOCK_DEVICES=3100 && echo "▸ Fake modules :3100-3106 (Shelly, Hue, Trydan, Renson, Tasmota, ESPHome, Forecast.Solar)"
  (cd server && MOCK_PORT=3000 MOCK_CLOUD_PORT=3900 MOCK_DEVICES_PORT="${MOCK_DEVICES}" cargo run -q --bin mock-airzone) &
  PIDS+=($!)
  export BREEZONE_AIRZONE_CLOUD_URL="http://127.0.0.1:3900" BREEZONE_SCAN_SUBNET="127.0.0.0/30" BREEZONE_POLL_INTERVAL_SECS=5
fi

if [ "${SHIPPED}" = "true" ]; then
  # The same list as the image (Dockerfile, SHIPPED_MODULES).
  SHIPPED_MODULES="$(sed -n 's/^ARG SHIPPED_MODULES="\(.*\)"/\1/p' Dockerfile)"
  SHIPPED_DIR="${PROJECT_DIR}/data/dev-modules"
  echo "▸ Shipped modules (${SHIPPED_MODULES}) → ${SHIPPED_DIR}"
  rm -rf "${SHIPPED_DIR}" && mkdir -p "${SHIPPED_DIR}"
  # shellcheck disable=SC2046
  (cd modules && cargo build -q --release --target wasm32-wasip2 $(for m in ${SHIPPED_MODULES}; do printf -- '-p breezone-module-%s ' "$m"; done))
  for m in ${SHIPPED_MODULES}; do
    (cd server && cargo run -q --bin module-publish -- pack "../modules/$m/manifest.json" "../modules/target/wasm32-wasip2/release/breezone_module_$m.wasm" "${SHIPPED_DIR}") > /dev/null
  done
  rm -f "${SHIPPED_DIR}"/*.message
  export BREEZONE_MODULES_DEV="${SHIPPED_DIR}"
  # The fake devices answer on this machine: the modules may reach it (development builds only).
  [ "${FAKE}" = "true" ] && export BREEZONE_MODULES_LOOPBACK=1
fi

echo "▸ Breezone server :${DEV_PORT}"
(cd server && cargo run -q --bin breezone-server) &
PIDS+=($!)

until curl -sf "http://localhost:${DEV_PORT}/health" > /dev/null 2>&1; do sleep 1; done
if [ "${MODULES}" = "true" ] && [ "${FAKE}" = "true" ]; then
  ADMIN_USER="${BREEZONE_ADMIN_USER:-admin}" ADMIN_PASSWORD="${BREEZONE_ADMIN_PASSWORD:-motdepasse1}"
  # A new database: the admin first, as the first-run page would.
  curl -sf -X POST "http://localhost:${DEV_PORT}/api/setup" -H 'Content-Type: application/json' \
    -d "{\"username\":\"${ADMIN_USER}\",\"password\":\"${ADMIN_PASSWORD}\"}" > /dev/null 2>&1 || true
  python3 scripts/demo-modules.py "http://localhost:${DEV_PORT}" "${ADMIN_USER}" "${ADMIN_PASSWORD}" 3100 "${DATA_DIR}/breezone.db" || true
fi
echo "▸ Web UI (Vite) → http://localhost:5173"
(cd web && npm run dev -- --host) &
PIDS+=($!)
wait
