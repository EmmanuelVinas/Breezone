# syntax=docker/dockerfile:1
# Breezone: one small image with the Rust server, the fake Airzone (demo) and the web UI.

# --- Web UI (React + Vite) -----------------------------------------------------------------
FROM node:24-alpine AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# --- Server (Rust, static musl binary) -----------------------------------------------------
FROM rust:1-alpine AS server
RUN apk add --no-cache musl-dev gcc make
WORKDIR /src
# Build the dependencies first, with stub sources, so that they are cached.
COPY server/Cargo.toml server/Cargo.lock ./
RUN mkdir -p src/bin \
 && echo 'fn main() {}' > src/main.rs \
 && echo 'fn main() {}' > src/bin/mock_airzone.rs \
 && touch src/lib.rs \
 && cargo build --release --locked \
 && rm -rf src
COPY server/ ./
RUN touch src/main.rs src/lib.rs src/bin/mock_airzone.rs \
 && cargo build --release --locked --bins

# --- Runtime -------------------------------------------------------------------------------
FROM alpine:3.22
ARG VERSION=dev
LABEL org.opencontainers.image.title="Breezone" \
      org.opencontainers.image.description="Local control of Airzone HVAC systems: server, web UI and API for the mobile app. Not affiliated with Airzone." \
      org.opencontainers.image.source="https://github.com/EmmanuelVinas/breezone" \
      org.opencontainers.image.url="https://github.com/EmmanuelVinas/breezone" \
      org.opencontainers.image.licenses="LicenseRef-PolyForm-Noncommercial-1.0.0" \
      org.opencontainers.image.version="${VERSION}"
RUN apk add --no-cache ca-certificates tzdata \
 && adduser -D -H -u 10001 breezone \
 && mkdir -p /data /app \
 && chown breezone:breezone /data
COPY --from=server /src/target/release/breezone-server /src/target/release/mock-airzone /usr/local/bin/
COPY --from=web /web/dist /app/web
ENV BREEZONE_DATA_DIR=/data \
    BREEZONE_WEB_DIR=/app/web \
    BREEZONE_PORT=8427 \
    RUST_LOG=info,tower_http=warn,sqlx=warn
USER breezone
VOLUME /data
EXPOSE 8427
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s CMD wget -qO- http://127.0.0.1:8427/health >/dev/null || exit 1
ENTRYPOINT ["breezone-server"]
