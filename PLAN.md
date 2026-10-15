# PLAN.md — Milestone status & log

> Working document. Status lives here; the permanent tracker with checkboxes lives in SPEC.md §14.

## Current state

- **Date:** 2026-10-15 · **Spec:** v0.3 (decisions D1–D29)
- **Next milestone:** M2 — orgs, pools, provider keys (encrypted), quota/rate engine
- **Code:** M1 complete — gateway proxies OpenAI (stream + non-stream) with hashed pool-key auth, SSE-usage metering, dual-DB (SQLite+Postgres) via sqlx Any; CI green on `main`

## Milestone status

| M | Status | Notes |
|---|---|---|
| M0 | **done** (2026-10-14) | CI green: [run 38572086285](https://github.com/Phat1911/Tributary/actions/runs/38572086285) |
| M1 | **done** (2026-10-15) | CI green incl. dual-DB tests: [run 38682358525](https://github.com/Phat1911/Tributary/actions/runs/38682358525); local end-to-end smoke vs mock upstream ✓; real-OpenAI curl demo pending user key |
| M2–M6 | pending | V1 — Gateway + Org Pools |
| M7–M11 | pending | V2 — Root Pool |
| M12–M14 | pending | V3 — Operator network |

## Milestone log (append per completed milestone; keep the most recent 10)

Template:

```
### M<n> — <name> (date)
- changed: <paths>
- tests: <what ran, result>
- evidence: <SPEC "done when" check>
- limitations/notes: <known gaps>
```

### M1 — gateway core: proxy, pool keys, streaming, metering (2026-10-15)
- **changed:** `migrations/{0001_pool_keys.sql,0002_usage_events.sql}` · `crates/gateway/` (Cargo.toml, src/{main.rs,lib.rs,config.rs,state.rs,error.rs,keys.rs,db.rs,sse.rs,chat.rs,models_api.rs}, tests/{common/mod.rs,api.rs,streaming.rs}) · `.github/workflows/ci.yml` (postgres service) · `scripts/mock_openai.py` · `Cargo.lock`
- **tests:** local (Windows-cargo WSL interop) — `cargo fmt --check` ✓, `clippy --workspace --all-targets -- -D warnings` ✓, `cargo test --workspace` 22/22 ✓ (13 lib incl. 7 SSE-meter units, 6 api + 3 streaming integration, each run against SQLite **and** Docker Postgres 16) · local end-to-end smoke: real `serve`+`seed-key` binary + real `curl` vs Python mock upstream — non-stream proxy, stream strip/forward contract, 401 shape, 3 `usage_events` rows verified via `psql` · CI — [run 38682358525](https://github.com/Phat1911/Tributary/actions/runs/38682358525) green on `main`: Rust (fmt/clippy/test, dual-DB via postgres:16 service) ✓, dashboard ✓, cargo-deny ✓.
- **evidence:** SPEC §14 M1 "done when" = *`curl` chat via `sk-pool-…` key; usage row written; both DBs pass* — curl matrix done against the mock upstream locally (real key demo below), usage rows written and inspected in Postgres, both DBs pass in CI and locally. **Outstanding:** the manual demo with a **real OpenAI key** (needs the founder's `TRIBUTARY_OPENAI_API_KEY`; commands in the M1 report / repo scripts).
- **dependencies (AGENTS workflow step 10):** `axum` 0.8 + `tokio` (rt/signal) — SPEC §13-locked stack; `sqlx` 0.9 (runtime-tokio, any, sqlite, postgres, migrate, macros) — dual-DB requirement, Any keeps ONE portable query path (macros feature is required by the `migrate!` proc-macro); `reqwest` 0.12 (json, stream, **rustls**-tls — avoids native-TLS/OpenSSL link pain in the Windows-cargo interop) — upstream HTTP+streaming; `serde`/`serde_json` — OpenAI JSON; `sha2`+`subtle`+`hex` — hashed-key auth with constant-time compare; `rand` — key material/ids; `futures-util` — stream plumbing; `tracing`+`tracing-subscriber` (env-filter, json) — SPEC §13 observability; `thiserror`/`anyhow` — error plumbing. Dev: `tempfile` (sqlite tmp dbs), `tower` (oneshot router tests), `http-body-util` (body collection). No CLI framework: 3 fixed subcommands hand-parsed. cargo-deny clean with the existing allow-list (no deny.toml changes).
- **limitations/notes:** quota/rate-limit checks (rule 4) deferred to M2 with the quota engine — gateway enforces key validity/expiry only; `cost_micros` pinned 0 (pricing table D23 is M2+); streaming usage fallback counts content chunks (prompt=0) when an upstream ignores `stream_options.include_usage` (OpenAI supports it — fallback is for OpenAI-dialect oddballs); metering-insert failure after a streamed response can only be logged (response already sent) — AUDIT A-003; Windows-local tests use in-memory SQLite because sqlx-0.9 `Any` cannot express absolute Windows paths through its URL form (drive colon parses as port separator) — CI/Linux uses real tempfiles — AUDIT A-002; local dev via Windows-cargo interop still needs `WSLENV=CARGO_INCREMENTAL CARGO_INCREMENTAL=0`; DB queries live in `crates/gateway/src/db.rs` as fully portable SQL + a mechanical `?`→`$n` PG rewrite (sqlx Any does not unify placeholders) — revisit placement when the M2 control plane shares queries; OpenAI-SDK contract tests in CI activate at M5 (director ruling, SPEC §14 M5 row).

### M0 — monorepo scaffold + CI skeleton (2026-10-14)
- **changed:** `Cargo.toml`, `Cargo.lock`, `crates/{gateway,control,ledger,crypto,common}/`, `pnpm-workspace.yaml`, `package.json`, `pnpm-lock.yaml`, `apps/dashboard/`, `.github/workflows/ci.yml`, `deny.toml`, `.gitignore`, `LICENSE`, `README.md`
- **tests:** local — `cargo fmt --all -- --check` ✓, `cargo clippy --workspace --all-targets -- -D warnings` ✓, `cargo test --workspace` 5/5 ✓, `pnpm install` + `pnpm -C apps/dashboard build` ✓, `tsc --noEmit` ✓. CI — [run 38572086285](https://github.com/Phat1911/Tributary/actions/runs/38572086285) green on `main`: rust ✓, dashboard ✓, cargo-deny ✓.
- **evidence:** SPEC §14 M0 "done when" = *PR pipeline green on empty hello-world crates* — pipeline green at `b000c42` on push to `main` (same workflow runs on PRs).
- **dependencies (AGENTS workflow step 10):** Rust — **none** (zero external crates). JS — `next@^16.4.0`, `react`/`react-dom@^19.3.0`, `typescript@^7.0.2`, `@types/{node,react,react-dom}`: the minimal Next.js + TS app M0 requires; **no** Tailwind/shadcn (kickoff decision — arrive with M3 dashboards). pnpm pinned `10.15.1` via `packageManager`. cargo-deny license allow-list = permissive SPDX set + this repo's own `FSL-1.1-ALv2`.
- **limitations/notes:** crates are placeholder libs (`crate_name()` + one unit test each); crate names carry the `tributary-` prefix ahead of crates.io reservation (D19); dashboard is a TS-only placeholder page; `next build` auto-adjusted `tsconfig.json` (`jsx: react-jsx`, `.next/dev/types` include) — kept; local Rust builds on this machine run via Windows-cargo WSL interop (needs `WSLENV=CARGO_INCREMENTAL CARGO_INCREMENTAL=0`), or just use CI.

## Fast jump — most recent milestone → changed files

**M1 (2026-10-15):**

- Migrations: `migrations/0001_pool_keys.sql` · `migrations/0002_usage_events.sql`
- Gateway: `crates/gateway/Cargo.toml` · `crates/gateway/src/{main.rs,lib.rs,config.rs,state.rs,error.rs,keys.rs,db.rs,sse.rs,chat.rs,models_api.rs}` · `crates/gateway/tests/{common/mod.rs,api.rs,streaming.rs}`
- CI: `.github/workflows/ci.yml` (postgres:16 service + `TRIBUTARY_TEST_DATABASE_URL`)
- Scripts: `scripts/mock_openai.py`
- Lockfile: `Cargo.lock`

**M0 (2026-10-14):**

- Root: `Cargo.toml` · `Cargo.lock` · `pnpm-workspace.yaml` · `package.json` · `pnpm-lock.yaml` · `deny.toml` · `.gitignore` · `LICENSE` · `README.md`
- Rust: `crates/gateway/{Cargo.toml,src/lib.rs}` — and the same pair under `crates/control/`, `crates/ledger/`, `crates/crypto/`, `crates/common/`
- Web: `apps/dashboard/{package.json,tsconfig.json,next.config.ts,next-env.d.ts}` · `apps/dashboard/app/{layout.tsx,page.tsx,globals.css}`
- CI: `.github/workflows/ci.yml`
