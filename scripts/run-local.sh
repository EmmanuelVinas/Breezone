#!/usr/bin/env bash
# Starts Breezone with docker compose.
#
#   ./scripts/run-local.sh            # build + start, asks whether to wipe the database
#   ./scripts/run-local.sh --demo     # + fake Airzone WebServer and fake Airzone Cloud
#   ./scripts/run-local.sh --tunnel   # + Cloudflare Tunnel (CLOUDFLARE_TUNNEL_TOKEN in .env)
#   ./scripts/run-local.sh --reset    # wipe the database without asking
#   ./scripts/run-local.sh --logs     # follow the logs once started
#
# A Breezone already running (container or local dev server) is stopped first.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="${SCRIPT_DIR}/.."

cd "${PROJECT_DIR}"

DEMO=false
TUNNEL=false
RESET=ask
FOLLOW_LOGS=false
for arg in "$@"; do
  case $arg in
    --demo) DEMO=true ;;
    --tunnel) TUNNEL=true ;;
    --reset) RESET=yes ;;
    --keep) RESET=no ;;
    --logs) FOLLOW_LOGS=true ;;
    -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

# ── Step 1: .env ───────────────────────────────────────────────────────────────────────────
if [ ! -f "${PROJECT_DIR}/.env" ]; then
  cp "${PROJECT_DIR}/.env.sample" "${PROJECT_DIR}/.env"
  echo "▸ .env created from .env.sample — review it (BREEZONE_SCAN_SUBNET, TZ…)"
fi
set -a
# shellcheck source=../.env
source "${PROJECT_DIR}/.env"
set +a

BREEZONE_HTTP_PORT="${BREEZONE_HTTP_PORT:-8080}"
PROFILES=()
if [ "${DEMO}" = "true" ]; then
  PROFILES+=(--profile demo)
  # Point Breezone to the fake Airzone WebServer and the fake Airzone Cloud.
  export BREEZONE_AIRZONE_HOSTS="Maison=mock-airzone:3000"
  export BREEZONE_AIRZONE_CLOUD_URL="http://mock-airzone:3900"
fi
if [ "${TUNNEL}" = "true" ]; then
  if [ -z "${CLOUDFLARE_TUNNEL_TOKEN:-}" ]; then
    echo "CLOUDFLARE_TUNNEL_TOKEN is empty in .env — create a tunnel on Cloudflare first (see README)." >&2
    exit 1
  fi
  PROFILES+=(--profile tunnel)
fi
COMPOSE=(docker compose -f docker-compose.yml -f docker-compose.build.yml ${PROFILES[@]+"${PROFILES[@]}"})

# ── Step 2: Docker ─────────────────────────────────────────────────────────────────────────
if ! docker info > /dev/null 2>&1; then
  echo "Docker is not running." >&2
  exit 1
fi

# ── Step 3: Database ───────────────────────────────────────────────────────────────────────
DATA_VOLUME="breezone_breezone_data"
# The project was called FreeZone before: reuse its data volume once.
LEGACY_VOLUME="freezone_freezone_data"
if ! docker volume inspect "${DATA_VOLUME}" > /dev/null 2>&1 && docker volume inspect "${LEGACY_VOLUME}" > /dev/null 2>&1; then
  echo "▸ Copying the data of the former FreeZone installation…"
  docker rm -f freezone > /dev/null 2>&1 || true
  docker volume create "${DATA_VOLUME}" > /dev/null
  docker run --rm -v "${LEGACY_VOLUME}:/from:ro" -v "${DATA_VOLUME}:/to" alpine:3.22 sh -c 'cp -a /from/. /to/'
fi
if docker volume inspect "${DATA_VOLUME}" > /dev/null 2>&1; then
  if [ "${RESET}" = "ask" ]; then
    read -r -p "Reset the database? This deletes accounts, installations, routines and history [y/N]: " ANSWER
    [[ "${ANSWER}" =~ ^[Yy]$ ]] && RESET=yes || RESET=no
  fi
  if [ "${RESET}" = "yes" ]; then
    echo "▸ Resetting the database…"
    "${COMPOSE[@]}" down --remove-orphans > /dev/null 2>&1 || true
    docker volume rm "${DATA_VOLUME}" > /dev/null
    echo "  Database reset — the setup page will ask for a new administrator."
  fi
else
  echo "▸ No database yet: it will be created on first start."
fi

# ── Step 4: Build ──────────────────────────────────────────────────────────────────────────
# Built before stopping anything: a failed build leaves the running server untouched.
printf "▸ Building"
[ "${DEMO}" = "true" ] && printf " (demo: fake Airzone)"
[ "${TUNNEL}" = "true" ] && printf " (Cloudflare Tunnel)"
echo "…"
"${COMPOSE[@]}" build

# ── Step 5: Stop the Breezone already running ──────────────────────────────────────────────
# Containers of this project (any profile), a container named "breezone" started another way,
# and local development servers (scripts/dev.sh, cargo run) holding the port.
docker compose -f docker-compose.yml -f docker-compose.build.yml --profile demo --profile tunnel down --remove-orphans > /dev/null 2>&1 || true
docker rm -f freezone > /dev/null 2>&1 || true   # former name
if docker container inspect breezone > /dev/null 2>&1; then
  echo "▸ Stopping the running container « breezone »"
  docker rm -f breezone > /dev/null
fi
for PID in $(lsof -nP -t -iTCP:"${BREEZONE_HTTP_PORT}" -sTCP:LISTEN 2>/dev/null || true); do
  COMMAND=$(ps -p "${PID}" -o comm= 2>/dev/null || true)
  case "${COMMAND}" in
    *breezone-server*|*mock-airzone*)
      echo "▸ Stopping the local Breezone server (pid ${PID})"
      kill "${PID}" 2>/dev/null || true
      ;;
    *com.docke*|*docker*|"") ;;
    *)
      echo "Port ${BREEZONE_HTTP_PORT} is used by another program (${COMMAND}, pid ${PID})." >&2
      echo "Stop it or set BREEZONE_HTTP_PORT in .env." >&2
      exit 1
      ;;
  esac
done
for _ in $(seq 1 20); do
  lsof -nP -t -iTCP:"${BREEZONE_HTTP_PORT}" -sTCP:LISTEN > /dev/null 2>&1 || break
  sleep 0.5
done

# ── Step 6: Start ──────────────────────────────────────────────────────────────────────────
echo "▸ Starting…"
"${COMPOSE[@]}" up -d --remove-orphans

# ── Step 7: Wait for the server ────────────────────────────────────────────────────────────
printf "▸ Waiting for Breezone… "
START=$SECONDS
until curl -sf "http://localhost:${BREEZONE_HTTP_PORT}/health" > /dev/null 2>&1; do
  if [ $((SECONDS - START)) -gt 90 ]; then
    echo "failed — last log lines:"
    docker compose logs --tail 40 breezone
    exit 1
  fi
  sleep 1
done
echo "ready in $((SECONDS - START))s"

NEEDS_SETUP=$(curl -sf "http://localhost:${BREEZONE_HTTP_PORT}/api/setup" | grep -q '"needs_setup":true' && echo yes || echo no)
MEMORY=$(docker stats --no-stream --format '{{.MemUsage}}' breezone 2>/dev/null || true)

echo ""
echo "Breezone is running → http://localhost:${BREEZONE_HTTP_PORT}"
[ -n "${MEMORY}" ] && echo "  Memory: ${MEMORY}"
if [ "${NEEDS_SETUP}" = "yes" ]; then
  echo "  First start: open the URL above to create the administrator account."
fi
if [ "${DEMO}" = "true" ]; then
  echo "  Demo: the fake installation « Maison » is preconfigured."
  echo "        Settings › Compte Airzone: demo@breezone.local / demo-password to import its groups."
fi
echo "  Logs: docker compose logs -f breezone   ·   Stop: docker compose ${PROFILES[*]:-} down"

if [ "${FOLLOW_LOGS}" = "true" ]; then
  "${COMPOSE[@]}" logs -f
fi
