# Tributary

**Small streams. Pooled power.**

**Status: v0.1.0 — self-hostable AI token pools** (gateway, org pools,
dashboard). Pre-alpha software, real release: every org gets per-user
keys over pooled provider capacity, with quotas, audit, and leak
signals. Roadmap lives in [`SPEC.md`](SPEC.md) §14.

## Quickstart

> **Which terminal?** Run these in a **POSIX shell** — Linux, macOS, or
> **WSL on Windows** (enable Docker Desktop's WSL integration, check
> with `docker ps`) — and paste the block as-is. **Windows PowerShell**
> works line-by-line instead: split the first line at `&&`, use
> `Copy-Item deploy\.env.example .env`, drop the trailing `\`
> continuations, and generate the key in WSL/Git Bash
> (`openssl rand -hex 32`). **CMD is not supported.**

```sh
git clone https://github.com/Phat1911/Tributary.git && cd Tributary
cp deploy/.env.example .env   # set TRIBUTARY_MASTER_KEY=<output of: openssl rand -hex 32>
docker compose up -d          # prefers the prebuilt image; builds from source only as fallback
docker exec -it -e TRIBUTARY_DATABASE_URL=sqlite:///data/tributary.db \
  tributary tributary create-admin --email you@example.com --password '<generated>'
```

**Login at http://localhost:8111** — time to first login is **~2 minutes
with the prebuilt image** (the first `up` pulls ~110 MB); **~10–15
minutes if it builds from source** instead (cold Rust + dashboard
compile).

> **Trusted-proxy note:** if you front Tributary with a reverse proxy,
> set `TRIBUTARY_TRUSTED_PROXIES` to that proxy's CIDR — IP-derived
> controls (allowlists, sharing detection) are only sound then; without
> it, X-Forwarded-For is ignored and the socket address is used. Details:
> [`deploy/README.md`](deploy/README.md).

## What it is

A tributary is a small stream that feeds a larger river — and that is
the whole design: members' small, governed streams of AI usage flowing
into one pooled upstream capacity. Each member gets their own API key;
every request is metered, quota-checked, and IP-fingerprinted at the
gateway **before** any upstream call. The pool's owner holds the
provider keys (envelope-encrypted at rest) and decides who draws how
much; odd signals — impossible travel, key sharing across countries —
surface as review flags, not silent surprises.

## Where to go next

| | |
|---|---|
| [`deploy/README.md`](deploy/README.md) | Detailed ops: ports, reverse proxies (the trusted-proxy setting matters), backups, Postgres, healthchecks, quota behavior |
| [`SPEC.md`](SPEC.md) | Architecture, security/economic rules, milestone roadmap |
| [`SECURITY.md`](SECURITY.md) | Private disclosure by email; finder credit |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Conventions (teaching comments, dual-DB rule) and dev setup |
| [`LICENSE`](LICENSE) | FSL-1.1-ALv2 — each version converts to Apache-2.0 after two years |

## Repository layout

```
crates/            Rust workspace (gateway, control, ledger, crypto, common)
apps/dashboard/    Next.js + TypeScript dashboard (pnpm workspace)
deploy/            Dockerfile, entrypoint, .env.example — single-container deploy
docker-compose.yml the quickstart deployment (root; reads a gitignored .env)
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

CI (`.github/workflows/ci.yml`) runs all of the above, `cargo-deny`,
and the OpenAI SDK contract test.

## License

Copyright 2026 Tran Dinh Hong Phat. Licensed under the
[Functional Source License, Version 1.1, ALv2 Future License](LICENSE)
(FSL-1.1-ALv2). Each version converts to Apache-2.0 two years after it is
made available.
