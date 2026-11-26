# syntax=docker/dockerfile:1
# Breezone: one small image with the Rust server, the fake Airzone (demo) and the web UI.

# --- Web UI (React + Vite) -----------------------------------------------------------------
# Static files, identical for every platform: built once, natively.
FROM --platform=$BUILDPLATFORM node:24-alpine AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# --- Modules that come with Breezone (WebAssembly, the same for every platform) ------------
# Built once, natively; packed by the server's `module-publish` below. They are trusted like the
# binary they come with (the image, or the signed archive of a release).
FROM --platform=$BUILDPLATFORM rust:1-alpine AS modules
RUN rustup target add wasm32-wasip2
WORKDIR /src
COPY server/contracts/wit server/contracts/wit
COPY modules/ modules/
# The modules shipped with Breezone (each replaces the driver compiled in with the same id).
ARG SHIPPED_MODULES="tasmota shelly trydan forecast_solar reolink hoymiles anker deye tplink tydom hue renson rte weather linky netatmo ezviz anker_cloud esphome homekit airzone_local"
# The registry and the build folder are caches kept by BuildKit (and by the CI, see the workflows):
# only what changed is compiled again. The crates are downloaded by one build at a time (`locked`:
# the images of both architectures are built at once, and share the registry), then unpacked by
# each build in a folder of its own (`registry/src`: the modules and the server are compiled at
# the same time, and unpacking a crate twice at once fails).
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    cd modules && cargo fetch --locked
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-src-modules,target=/usr/local/cargo/registry/src,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=modules-target,target=/src/modules/target \
    cd modules \
 && cargo build --release --locked --target wasm32-wasip2 $(for m in $SHIPPED_MODULES; do printf -- '-p breezone-module-%s ' "$m"; done) \
 && for m in $SHIPPED_MODULES; do \
      mkdir -p "/out/$m" && cp "target/wasm32-wasip2/release/breezone_module_$m.wasm" "$m/manifest.json" "/out/$m/"; \
    done

# --- Server (Rust, static musl binary) -----------------------------------------------------
FROM rust:1-alpine AS server
ARG TARGETARCH
RUN apk add --no-cache musl-dev gcc make
WORKDIR /src
COPY server/ ./
# The reference of the API, embedded for the MCP server (`include_str!("../../../docs/API.md")`).
COPY docs/API.md /docs/API.md
# The registry and the build folder are caches kept by BuildKit (and by the CI): a new version, or
# a change of the server, compiles only what changed; the dependencies stay compiled. The binaries
# are copied out of the cache, which is not part of the image. The crates are downloaded by one
# build at a time and unpacked in a folder of its own (see the modules).
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    cargo fetch --locked
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-src-server-${TARGETARCH},target=/usr/local/cargo/registry/src,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=server-target-${TARGETARCH},target=/src/target \
    cargo build --release --locked --bins \
 && mkdir -p /src/out \
 && cp target/release/breezone-server target/release/mock-airzone target/release/module-publish /src/out/ \
 && /src/out/module-publish host > /src/host.json
COPY --from=modules /out/ /src/modules-in/
RUN mkdir -p /src/modules-out \
 && for m in /src/modules-in/*/; do \
      /src/out/module-publish pack "$m/manifest.json" "$m"/*.wasm /src/modules-out; \
    done \
 && rm -f /src/modules-out/*.message

# --- Release files (self-update, issue #40) ------------------------------------------------
# The server and the web UI only, exported by the release workflow to make the signed archive
# that installations download (`--target release-files --output type=local`).
FROM scratch AS release-files
COPY --from=server /src/out/breezone-server /breezone-server
COPY --from=web /web/dist /web
# What this version hosts for modules: read before updating, to check the modules installed.
COPY --from=server /src/host.json /host.json
COPY --from=server /src/modules-out /modules

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
COPY --from=server /src/out/breezone-server /src/out/mock-airzone /usr/local/bin/
COPY --from=web /web/dist /app/web
COPY --from=server /src/modules-out /app/modules
# Breezone's licence; those of the software it includes are in the web UI (third-party-licenses.txt).
COPY LICENSE.md /app/LICENSE.md
ENV BREEZONE_DATA_DIR=/data \
    BREEZONE_WEB_DIR=/app/web \
    BREEZONE_PORT=8427 \
    RUST_LOG=info,tower_http=warn,sqlx=warn
USER breezone
VOLUME /data
EXPOSE 8427
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s CMD wget -qO- http://127.0.0.1:8427/health >/dev/null || exit 1
ENTRYPOINT ["breezone-server"]
