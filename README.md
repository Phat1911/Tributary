# Tributary

**Small streams. Pooled power.**

Self-hostable AI token pools — govern your org's AI usage with per-member keys,
quotas, and leak detection; or deposit surplus credits into a Solana-settled
marketplace.

> **Status:** pre-alpha (milestones M0–M4 landed; M5 — packaging, docs,
> hardening — in flight). See [`SPEC.md`](SPEC.md) §14 for the roadmap.

## Deploy in one command

```sh
git clone https://github.com/Phat1911/Tributary.git && cd Tributary
cp deploy/.env.example .env
# set the one required secret — paste the output of: openssl rand -hex 32
$EDITOR .env          # TRIBUTARY_MASTER_KEY=<that output>
docker compose up -d --build
```

- Gateway + dashboard: **http://localhost:8111** (host port 8111 →
  container 8080; see the note in `docker-compose.yml` if you already use
  host 8080).
- First admin, out-of-band by design:

  ```sh
  docker exec -it \
    -e TRIBUTARY_DATABASE_URL=sqlite:///data/tributary.db \
    tributary tributary create-admin --email you@example.com --password '<generated>'
  ```

- SQLite state lives in one file (`/data/tributary.db`); upgrades are
  `git pull && docker compose up -d --build` — migrations run on boot.

> **Trusted-proxy warning:** IP-derived controls (allowlists, sharing
> detection) are only sound when Tributary runs behind a reverse proxy you
> control **AND** `TRIBUTARY_TRUSTED_PROXIES` is set to that proxy's CIDR;
> without it, X-Forwarded-For is ignored and the socket address is used.

Full guide — ports, proxies, backups, Postgres, healthchecks:
[`deploy/README.md`](deploy/README.md). Found something exploitable? It
goes to [`SECURITY.md`](SECURITY.md), never the issue tracker.

## Repository layout

```
crates/            Rust workspace (gateway, control, ledger, crypto, common)
apps/dashboard/    Next.js + TypeScript dashboard (pnpm workspace)
deploy/            Dockerfile, entrypoint, .env.example — single-container deploy
docker-compose.yml the one-command deployment (root; reads a gitignored .env)
```

## Developing

Rust:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Dashboard:

```sh
pnpm install
pnpm -C apps/dashboard build
```

CI (`.github/workflows/ci.yml`) runs all of the above plus `cargo-deny`.

## License

Copyright 2026 Tran Dinh Hong Phat. Licensed under the
[Functional Source License, Version 1.1, ALv2 Future License](LICENSE)
(FSL-1.1-ALv2). Each version converts to Apache-2.0 two years after it is
made available.
