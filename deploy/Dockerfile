# syntax=docker/dockerfile:1
# Tributary single-container image (M5 packaging, SPEC §13/§14 M5).
#
# Multi-stage, multi-arch (linux/amd64 + linux/arm64 via buildx + QEMU —
# every dependency compiles from source: rustls/ring and bundled SQLite
# need only a C compiler, no OpenSSL, no cmake, so the same Dockerfile
# builds natively on both architectures).
#
# Build context is the REPO ROOT (docker-compose.yml sets
# `context: . dockerfile: deploy/Dockerfile`), because stages need both
# apps/ and crates/.

# ---- Stage 1: dashboard static export -----------------------------------
# Produces `out/`: a fully static Next.js export. The dashboard is a pure
# client of the control API (same-origin /api/v1), so no Node server is
# needed at runtime — whatever fronts the deployment serves these files.
FROM node:22-slim AS dashboard

# Corepack activates the pnpm version pinned in the root package.json
# (`packageManager: pnpm@10.15.1`); the env var keeps it non-interactive
# in CI. No global npm installs, no drift from the lockfile's pnpm.
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN corepack enable pnpm

WORKDIR /build

# Manifests first for layer caching: the expensive dependency install
# only re-runs when a lockfile/manifest changes.
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY apps/dashboard/package.json apps/dashboard/package.json
RUN pnpm install --frozen-lockfile

# Then the dashboard source + build.
COPY apps/dashboard apps/dashboard
ENV NEXT_TELEMETRY_DISABLED=1
# Static export (TRIBUTARY_STATIC_EXPORT=1 flips next.config.ts to
# `output: "export"` and skips the dev rewrites). KNOWN M5 phase-2a
# blocker: `app/org/pools/[id]/page.tsx` is a dynamic segment without
# `generateStaticParams`, which `output: "export"` rejects. Do not hack
# the page in packaging — the route must move to a static-exportable
# pattern (client-side query param) or export an empty
# `generateStaticParams` with an edge fallback, tracked in deploy/README.
RUN pnpm -C apps/dashboard build:export

# ---- Stage 2: Rust release binary ----------------------------------------
# rust:1.98-slim ships gcc (needed to link and to build ring/libsqlite3);
# rustls (no native TLS) and sqlx's bundled SQLite need nothing else.
# rust:1-slim tracks current stable (local toolchain 1.99.1 at M5);
# Cargo.lock (--locked below) pins the dependency graph regardless.
FROM rust:1-slim AS build
WORKDIR /build

# migrations/ is required at COMPILE time: `sqlx::migrate!` embeds the
# directory (crates/common/src/db.rs), so the runtime image needs no
# migration files.
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
RUN cargo build --release --locked -p tributary-gateway

# ---- Stage 3: runtime ------------------------------------------------------
# Only the release binary + the static dashboard + a /data volume dir for
# SQLite. No sources, no toolchains, and NO SECRETS baked in — the master
# key and every credential enter via environment at run time.
FROM debian:bookworm-slim AS runtime

# curl: compose healthcheck (and operator debugging); ca-certificates:
# upstream HTTPS calls from the gateway.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home-dir /data --shell /usr/sbin/nologin tributary

COPY --from=build /build/target/release/tributary-gateway /usr/local/bin/tributary
COPY --from=dashboard /build/apps/dashboard/out /usr/share/tributary/dashboard
COPY deploy/entrypoint.sh /usr/local/bin/entrypoint.sh
# 0755, not just +x: COPY preserves the source mode, and a SHELL script
# must be READABLE by the runtime user (711 works for binaries, not
# scripts — /bin/sh opens the file). The repo file is 755 too; this makes
# the image robust to umask/checkout quirks either way.
RUN chmod 0755 /usr/local/bin/entrypoint.sh

# SQLite home. Named volumes (docker-compose `volumes:`) inherit this
# directory's ownership; bind-mounted host dirs must be writable by the
# `tributary` user (uid 999+ — see deploy/README.md).
RUN mkdir -p /data && chown tributary:tributary /data
VOLUME /data

USER tributary
WORKDIR /data
EXPOSE 8080

# Subcommands: `tributary serve | migrate | create-admin`; with no args
# the entrypoint runs `migrate` then `serve` (see entrypoint.sh).
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
