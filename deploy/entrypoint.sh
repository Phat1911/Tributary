#!/bin/sh
# Tributary container entrypoint (M5 packaging).
#
# Default behavior: apply pending migrations, then `exec serve` so the
# gateway is PID 1 and receives signals (graceful shutdown) directly.
# Any arguments passed to the container (`docker run … <cmd>`) are exec'd
# verbatim instead — that is how `create-admin` is run via `docker exec`/
# `docker run` without triggering a second `serve`.
#
# Environment (see deploy/.env.example for the annotated list):
#   TRIBUTARY_DATABASE_URL     default sqlite:///data/tributary.db
#   TRIBUTARY_MASTER_KEY       REQUIRED — 64 hex chars (openssl rand -hex 32)
#   TRIBUTARY_BIND             default 0.0.0.0:8080
#   TRIBUTARY_TRUSTED_PROXIES  optional — comma-separated CIDR list
#   TRIBUTARY_MODELS           optional — default model allowlist
#   TRIBUTARY_LOG_FORMAT       optional — "json" for structured logs

set -eu

# --- Fail fast on the master key: it must be set and be exactly 64 hex
# chars (32 bytes). Without it the gateway cannot decrypt provider keys,
# so starting "half-working" is worse than refusing to start.
if [ -z "${TRIBUTARY_MASTER_KEY:-}" ]; then
    echo "ERROR: TRIBUTARY_MASTER_KEY is not set." >&2
    echo "       Generate one with:  openssl rand -hex 32" >&2
    echo "       Put it in your .env file (see deploy/.env.example) and restart." >&2
    exit 1
fi
if ! printf '%s' "$TRIBUTARY_MASTER_KEY" | grep -Eq '^[0-9a-fA-F]{64}$'; then
    echo "ERROR: TRIBUTARY_MASTER_KEY must be 64 hex characters (32 bytes)," >&2
    echo "       e.g. the output of:  openssl rand -hex 32" >&2
    exit 1
fi

# --- Defaults (single source of truth for container defaults lives here).
: "${TRIBUTARY_DATABASE_URL:=sqlite:///data/tributary.db}"
: "${TRIBUTARY_BIND:=0.0.0.0:8080}"
# The dashboard static export baked into the image (empty in dev images).
: "${TRIBUTARY_STATIC_DIR:=/usr/share/tributary/dashboard}"
export TRIBUTARY_DATABASE_URL TRIBUTARY_BIND TRIBUTARY_STATIC_DIR

# --- Trusted proxies (optional): comma-separated CIDRs whose X-Forwarded-For
# headers are honored. When UNSET, client IP = raw socket address and
# X-Forwarded-For is ignored — IP-derived controls (allowlists, sharing
# detection) are only sound behind a reverse proxy you control with this
# set to that proxy's CIDR (A-019, M5 hardening).
if [ -n "${TRIBUTARY_TRUSTED_PROXIES:-}" ]; then
    # Cheap paste-error guard (each entry: v4/v6-ish CIDR or bare address).
    # Full parsing fails CLOSED inside the gateway (M4 garbage-CIDR rule).
    oldIFS="$IFS"
    IFS=','
    for cidr in $TRIBUTARY_TRUSTED_PROXIES; do
        if ! printf '%s' "$cidr" | grep -Eq \
            '^[0-9A-Fa-f:.]+(/[0-9]{1,3})?$'; then
            echo "ERROR: TRIBUTARY_TRUSTED_PROXIES entry '$cidr' does not look like a CIDR/address." >&2
            exit 1
        fi
    done
    IFS="$oldIFS"
    export TRIBUTARY_TRUSTED_PROXIES
fi

# --- Explicit command override (e.g. `docker run … tributary create-admin …`).
if [ "$#" -gt 0 ]; then
    exec "$@"
fi

# --- Default path: migrate, then serve. Migrations are idempotent and run
# on every boot, which is also the upgrade path (`docker compose up -d`
# with a newer image applies pending migrations before serving).
echo "applying database migrations (${TRIBUTARY_DATABASE_URL%%\?*})…" >&2
tributary migrate

echo "starting tributary on ${TRIBUTARY_BIND}" >&2
exec tributary serve
