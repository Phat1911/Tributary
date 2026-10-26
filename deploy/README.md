# Deploying Tributary

This directory packages the single-container Tributary deployment: the
Rust binary (`serve` + `migrate` + `create-admin`), the statically
exported dashboard, and SQLite on a `/data` volume — one Docker command,
no database server required.

The fastest path lives in the repo-root [`README.md`](../README.md)
("Deploy in one command"). This guide is the complete version: ports,
proxies, backups, admins, upgrades.

## What you need

- Docker Engine with the `docker compose` plugin (v2).
- A machine you can leave running (a small VPS is plenty — SQLite has no
  separate server to feed).
- 10 minutes.

## Quick start

From a fresh clone:

```sh
cp deploy/.env.example .env
$EDITOR .env                # or: nano .env
# Inside .env, the ONLY mandatory edit:
#   TRIBUTARY_MASTER_KEY=<output of: openssl rand -hex 32>
docker compose up -d --build
```

Compose builds the image from [`deploy/Dockerfile`](Dockerfile) and starts
one service, `tributary`. **Nothing secret is baked into the image** — the
master key and every credential are read from the environment at run time.

### Ports — read this if you already run something on 8080

The compose file publishes **host port 8111 → container port 8080**.
Host 8080 is deliberately NOT used: it is the most commonly occupied port
on dev machines and homelabs, and port conflicts are the #1 "why won't
my container start" report. To change the host port, edit the
left side of `ports:` in [`docker-compose.yml`](../docker-compose.yml):

```yaml
    ports:
      - "8111:8080"   # host:container — container side stays 8080
```

Inside the container the gateway always binds `0.0.0.0:8080`
(`TRIBUTARY_BIND`, override only if you know why).

## Creating the first admin

The API refuses to serve admin traffic until a host admin exists, and the
first one is deliberately created out-of-band:

```sh
# -e needed: `docker exec` does NOT inherit env_file defaults — the
# entrypoint (which sets TRIBUTARY_DATABASE_URL) only runs for PID 1.
docker exec -it \
    -e TRIBUTARY_DATABASE_URL=sqlite:///data/tributary.db \
    tributary tributary create-admin \
  --email you@example.com --password 'a-long-generated-password'
```

(`docker exec` runs the command in the running container; the entrypoint
is not involved. Further users are created via the admin API.)

## Pointing the dashboard at it

The dashboard is baked into the image as static files
(`/usr/share/tributary/dashboard`, built by `pnpm build:export` in image
stage 1). It talks **same-origin** to `/api/v1` — there is no dashboard
URL to configure; whatever origin serves the API serves the UI:

- With the single container exposed directly: **http://localhost:8111**
- Behind your own reverse proxy: proxy `/api/v1/*` to the container and
  serve the static files from the same origin.

## Sharing ladder — who can reach your instance

Four rungs, smallest first. The container never changes — only the
network around it does. Climb as far as your group actually needs.

### Rung 1 — just this machine (localhost)

The default. `docker compose up -d`, then log in at
http://localhost:8111. Nothing to configure, nothing exposed.

### Rung 2 — your LAN

For a trusted local network (home, office):

1. Compose already publishes host port 8111 on every interface —
   nothing to change in the compose file.
2. Find the host's LAN address: `ip addr` on Linux (look for
   `192.168.x.x` / `10.x.x.x`), or the router's device list.
3. If the host runs a firewall, open the port:
   `sudo ufw allow 8111/tcp` (or your firewall's equivalent).
4. Everyone on the LAN uses `http://<that-address>:8111`.

No proxy is involved, so leave `TRIBUTARY_TRUSTED_PROXIES` unset —
the socket address IS each client's real LAN address, which is exactly
what allowlists and sharing detection want to see.

### Rung 3 — your tailnet (Tailscale)

For a private group across the internet — **no public IP, no domain,
no TLS certificates**. [Tailscale](https://tailscale.com) builds a
private encrypted network (a "tailnet") over your existing internet;
devices see each other as if on one LAN. A free plan covers small
personal tailnets; you sign in with an existing account (Google,
GitHub, …) — no payment involved.

Never used Tailscale? The whole setup is about ten minutes:

1. **Install it on the host** (the machine running Docker):
   Linux: `curl -fsSL https://tailscale.com/install.sh | sh` —
   macOS/Windows: the installer from
   [tailscale.com/download](https://tailscale.com/download).
2. **Bring it up**: `sudo tailscale up`, follow the printed link, sign
   in. Name the machine something findable, e.g. `tributary-box`.
3. **Let tailnet traffic reach port 8111** if the host runs a
   firewall: `sudo ufw allow in on tailscale0` (`tailscale0` is the
   interface Tailscale adds).
4. **Find the host's tailnet address**: in the admin console at
   [login.tailscale.com/admin/machines](https://login.tailscale.com/admin/machines)
   — every tailnet device gets a stable address in the `100.x.y.z`
   range. That address never changes; write it down.
5. **Install Tailscale on each member's device** and sign in. Either
   invite them to your tailnet (Users tab), or — better for a loose
   group — use **node sharing**: on the host's row in the admin
   console, "Share" → enter their address. Shared members can reach
   this one machine only, not your other devices.
6. **Use it**: with Tailscale connected, everyone opens
   `http://100.x.y.z:8111` — browser, curl, SDK base URL, all the
   same. Traffic is encrypted end-to-end (WireGuard) by Tailscale
   itself, which is why no HTTPS setup is needed for a private group.

**The trust note (read this):** tailnet traffic reaches the container
directly — there is no reverse proxy in the path. So leave
`TRIBUTARY_TRUSTED_PROXIES` **unset**: the socket address is the
caller's real tailnet address, which is what IP-derived controls
should see. Do **not** add `100.64.0.0/10` (the tailnet range) to the
trusted list — that would tell the gateway to honor
`X-Forwarded-For` headers from any tailnet device, and any member
could then spoof their way past an IP allowlist with one header line.

**Doctrine (D35):** founder tailnets are never a production
dependency — this rung is a self-host option for a private group,
nothing more. Tributary itself runs identically on all four rungs.

### Rung 4 — a public VPS

The guide's default topology: a small VPS, ports published, and your
own reverse proxy with TLS in front (Caddy or nginx + a certificate).
The proxy changes what the gateway should trust — that is exactly
what the next section is about.

## Trusted proxies — the one security setting that matters

> **IP-derived controls (allowlists, sharing detection) are only sound
> when Tributary runs behind a reverse proxy you control AND
> `TRIBUTARY_TRUSTED_PROXIES` is set to that proxy's CIDR; without it,
> X-Forwarded-For is ignored and the socket address is used.**

Why: any client can *send* an `X-Forwarded-For` header. If the gateway
trusted it blindly, a member could spoof their way past an IP allowlist
with one header line. The rule (M5 hardening, AUDIT A-019): the header is
honored only when the direct peer is a listed trusted proxy; otherwise the
client IP is the raw socket address — which, exposed directly to the
internet, is also fine for allowlists (just don't put the container's
internal Docker-network CIDR in the list).

```sh
# In .env — e.g. your nginx container on the compose network:
TRIBUTARY_TRUSTED_PROXIES=172.18.0.0/16
```

## Backups — it's one file

Single-container SQLite means the entire deployment state (orgs, keys,
usage metering, audit log) is **one file: `/data/tributary.db`** (plus its
`-wal`/`-shm` siblings). Back it up and you can always restore.

Safe backup (quiesces writes — the honest way on a running gateway):

```sh
docker compose stop          # or schedule during a quiet window
docker cp tributary:/data/tributary.db ./tributary-$(date +%F).db
docker compose start
```

With a named volume (the compose default) the same works on the host side
via the volume mount point. Copying the live file mid-write can capture a
torn snapshot across the WAL — if you need hot backups, switch to the
Postgres deployment and use `pg_dump`.

**Store the backup and your `TRIBUTARY_MASTER_KEY` apart from each other.**
The database alone is useless without the key (provider keys are envelope-
encrypted under it); the key alone is useless without the database. An
attacker needs both.

## Upgrading

```sh
git pull
docker compose pull || docker compose build
docker compose up -d
```

Migrations run **on every boot** (the entrypoint runs `tributary migrate`
before `serve`), so a newer image upgrades the schema automatically before
serving. SQLite makes no promises about downgrades — keep the backup you
took before upgrading.

## Postgres instead of SQLite

SQLite is the right default for one container. If you outgrow it
(concurrent writers), point `TRIBUTARY_DATABASE_URL` at Postgres in `.env`
and add a `postgres` service to your compose file; the binary and every
migration work identically on both (the repo's CI tests both on every
push).

## Quota behavior — how requests are counted

Tributary meters per-member usage with **reserve → settle → release**
semantics: before a request reaches the provider, the gateway RESERVES an
estimated token cost against the member's quota window; when the response
arrives it SETTLES the actual usage and RELEASES the overestimate.

The estimate (disclosed here per the product's no-surprises rule, A-020):

```
estimated_tokens = ceil(prompt_characters / 4) + max_completion_tokens
```

- `max_completion_tokens` comes from the request; **if the caller does not
  declare it, 256 is assumed.**
- A member can be denied **at reserve time** (before any upstream call, so
  no provider cost) when the estimate alone exceeds the remaining budget.
  That 429 explains itself: it names the estimate, the formula, the
  remaining window, and tells the caller to declare `max_tokens` to lower
  the estimate. This is deliberately DISTINCT from a usage-cap denial,
  which reads `Limit reached: …`.
- Settlement refunds the difference, so a conservative estimate never
  permanently consumes quota — only the actual usage is charged.

## Healthcheck

`docker-compose.yml` healthchecks `http://127.0.0.1:8080/healthz` inside
the container. `/healthz` ships with the M5 binary; until that phase
lands, the compose file documents a TCP-check fallback in a comment (swap
the `test:` line). The healthcheck marks the container `unhealthy` — it
does not restart it (`restart: unless-stopped` handles crashes).

## Packaging notes (M5)

The two original phase-2a gaps are closed:

1. The dashboard's one dynamic route (`/org/pools/[id]`) moved to a
   client-side query parameter (`/org/pools/?id=…`) so the static export
   (`output: "export"` + `trailingSlash`) succeeds — the page is a pure
   client of the control API either way.
2. The binary serves the export at `/` (via `TRIBUTARY_STATIC_DIR`, set
   by the container to `/usr/share/tributary/dashboard`) **and** answers
   `/healthz` (a DB-backed 200/503) — the compose healthcheck no longer
   needs a TCP fallback. SQLite also gained `create_if_missing` on
   connect, so a fresh `/data` volume boots without a pre-created
   database file.

## Manual `docker run` (no compose)

```sh
docker build -f deploy/Dockerfile -t tributary:local .
docker run -d --name tributary -p 8111:8080 \
  -e TRIBUTARY_MASTER_KEY=$(openssl rand -hex 32) \
  -v tributary-data:/data \
  --restart unless-stopped tributary:local
```
