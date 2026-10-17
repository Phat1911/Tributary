# PLAN.md — Milestone status & log

> Working document. Status lives here; the permanent tracker with checkboxes lives in SPEC.md §14.

## Current state

- **Date:** 2026-10-17 · **Spec:** v0.3 (decisions D1–D30)
- **Next milestone:** M3 — member self-service basics + dashboard shell
- **Code:** M2 complete — control plane (argon2 sessions, orgs/members/pools/keys/quota REST), envelope-encrypted provider keys, rule-4 quota gate, pool-scoped routing; dual-DB green locally and ready for CI

## Milestone status

| M | Status | Notes |
|---|---|---|
| M0 | **done** (2026-10-14) | CI green: [run 38572086285](https://github.com/Phat1911/Tributary/actions/runs/38572086285) |
| M1 | **done** (2026-10-15) | CI green incl. dual-DB tests: [run 38682358525](https://github.com/Phat1911/Tributary/actions/runs/38682358525); local end-to-end smoke vs mock upstream ✓; real-OpenAI curl demo pending user key |
| M2 | **done** (2026-10-17) | dual-DB 59/59 locally (SQLite in-memory + Docker Postgres 16 on :5433); done-when `m2_isolation.rs` green; CI run pending push |
| M3–M6 | pending | V1 — Gateway + Org Pools |
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

### M2 — orgs, pools, provider keys (encrypted), quota/rate engine (2026-10-17)
- **changed:** `migrations/{0003_orgs_users_sessions.sql,0004_pools_provider_keys_quota.sql,0005_usage_events_pool.sql}` · `crates/common/` (src/db.rs moved from gateway + keys.rs + time.rs, Cargo.toml) · `crates/crypto/` (full crate: MasterKey/seal/open) · `crates/control/` (full crate: Cargo.toml, src/{lib.rs,error.rs,auth.rs,db.rs,api.rs}, tests/{harness/mod.rs,api.rs}) · `crates/gateway/` (src/{main.rs,lib.rs,config.rs,state.rs,error.rs,keys.rs,db.rs,quota.rs,routing.rs,chat.rs,models_api.rs}, tests/{common/mod.rs,api.rs,streaming.rs,concurrency.rs,m2_isolation.rs}) · `AGENTS.md` (teaching-comments convention) · `SPEC.md` (D30 adoption sweep) · `Cargo.lock`
- **tests:** local (Windows-cargo WSL interop) — `cargo fmt --all` ✓, `clippy --workspace --all-targets -- -D warnings` ✓, `cargo test --workspace` **59/59 ✓** on the full dual-DB matrix: SQLite (in-memory, A-002 branch) **and** Postgres 16 (throwaway Docker container `tributary-pg-m2` on :5433, `TRIBUTARY_TEST_DATABASE_URL`, removed after) — 4 crypto lib · 13 common lib · 4 control lib · 7 control api · 12 gateway lib · 6 gateway api · 1 concurrency · 3 m2_isolation · 3 streaming. CI run expected on push (same workflow as M1: postgres:16 service, no CI changes needed).
- **evidence:** SPEC §14 M2 "done when" = *Two pools isolated: one's quota never affects the other* — `crates/gateway/tests/m2_isolation.rs::two_pools_isolated_token_quota_exhaustion_never_blocks_the_other`: pools A/B provisioned entirely through the control API (org → pool → provider key → member → issued `sk-pool-…` key); day-token quota 18 on A's member; A#1 serves (18 tokens metered), A#2 → **429** with `Retry-After` + "18 tokens per day … Resets at …" message and **zero upstream calls** (rule 4); B serves 3/3 with its own usage rows attributed (pool_id + provider_key_id); A's rows stay at 1. Companion tests: `request_rate_limit_fires_before_upstream_and_is_pool_scoped` (minute limit_requests on pool C: C#2 denied pre-upstream, D unaffected), `model_outside_allowlist_is_rejected_locally` (400, no upstream).
- **dependencies (AGENTS workflow step 10):** new this milestone — `chacha20poly1305` 0.10 (RustCrypto XChaCha20-Poly1305, **D30 director ruling**: pure Rust, no C toolchain in CI/Docker/multi-arch, zeroize-first-class) + `aead` (its trait); `argon2` 0.5 + `password-hash` (Argon2id PHC, D8); `zeroize` 1 (Zeroizing secret boundary, AGENTS secrets convention; Debug redacted by design). Promoted from gateway to common (no new externals): hex/rand/sha2/subtle/anyhow. Reused: axum/sqlx/reqwest(rustls)/serde/tracing/thiserror/tempfile/tower/http-body-util — all already justified in M0/M1. No deny.toml changes needed (all MIT/Apache-2.0/ISC, within allow-list). No CLI framework still (create-admin hand-parsed like serve/migrate).
- **limitations/notes:** sessions are **Bearer tokens** (24 h, stored SHA-256-hashed); cookies for the browser dashboard deferred to M3; **login has no brute-force throttling** — AUDIT A-004 (low, M3/M4); token quotas are consumption-based pre-checks (no prompt-size estimation) — a single call may overshoot its budget by one request (documented semantics: `limit ≤ used` denies BEFORE upstream; usage rows exist only for served requests, so denials never consume quota); provider-key selection is first-available (active + health≠invalid, by created_at) — health/weight-aware failover is M4+; key "health" is a single probe on save + manual revalidate (M2 LIGHT); pre-M1…M1 pool keys without `pool_id` are rejected at auth (documented pre-release behavior change; `seed-key` CLI removed, replaced by `create-admin` + API issuance); in-proc minute counters make rate limits process-local (single-binary SPEC §13 deployment) with DB counts as the authoritative backstop (D9); `COALESCE(SUM(...))` must be CAST to BIGINT for the sqlx Any driver (PG promotes to NUMERIC — fixed in cd5f97e, pattern to remember for future aggregates); shared-PG test DB bootstrap races tolerated in harnesses (loser logs in with shared creds); WSLENV for multi-var interop needs `:` separators (`WSLENV="CARGO_INCREMENTAL:TRIBUTARY_TEST_DATABASE_URL"`).

### M1 — gateway core: proxy, pool keys, streaming, metering (2026-10-15)
- **changed:** `migrations/{0001_pool_keys.sql,0002_usage_events.sql}` · `crates/gateway/` (Cargo.toml, src/{main.rs,lib.rs,config.rs,state.rs,error.rs,keys.rs,db.rs,sse.rs,chat.rs,models_api.rs}, tests/{common/mod.rs,api.rs,streaming.rs}) · `.github/workflows/ci.yml` (postgres service) · `scripts/mock_openai.py` · `Cargo.lock`
- **tests:** local (Windows-cargo WSL interop) — `cargo fmt --check` ✓, `clippy --workspace --all-targets -- -D warnings` ✓, `cargo test --workspace` 22/22 ✓ (13 lib incl. 7 SSE-meter units, 6 api + 3 streaming integration, each run against SQLite **and** Docker Postgres 16) · local end-to-end smoke: real `serve`+`seed-key` binary + real `curl` vs Python mock upstream — non-stream proxy, stream strip/forward contract, 401 shape, 3 `usage_events` rows verified via `psql` · CI — [run 38682358525](https://github.com/Phat1911/Tributary/actions/runs/38682358525) green on `main`: Rust (fmt/clippy/test, dual-DB via postgres:16 service) ✓, dashboard ✓, cargo-deny ✓.
- **evidence:** SPEC §14 M1 "done when" = *`curl` chat via `sk-pool-…` key; usage row written; both DBs pass* — curl matrix done against the mock upstream locally (real key demo below), usage rows written and inspected in Postgres, both DBs pass in CI and locally. **Outstanding:** the manual demo with a **real OpenAI key** (needs the founder's `TRIBUTARY_OPENAI_API_KEY`; commands in the M1 report / repo scripts).
- **dependencies (AGENTS workflow step 10):** `axum` 0.8 + `tokio` (rt/signal) — SPEC §13-locked stack; `sqlx` 0.9 (runtime-tokio, any, sqlite, postgres, migrate, macros) — dual-DB requirement, Any keeps ONE portable query path (macros feature is required by the `migrate!` proc-macro); `reqwest` 0.12 (json, stream, **rustls**-tls — avoids native-TLS/OpenSSL link pain in the Windows-cargo interop) — upstream HTTP+streaming; `serde`/`serde_json` — OpenAI JSON; `sha2`+`subtle`+`hex` — hashed-key auth with constant-time compare; `rand` — key material/ids; `futures-util` — stream plumbing; `tracing`+`tracing-subscriber` (env-filter, json) — SPEC §13 observability; `thiserror`/`anyhow` — error plumbing. Dev: `tempfile` (sqlite tmp dbs), `tower` (oneshot router tests), `http-body-util` (body collection). No CLI framework: 3 fixed subcommands hand-parsed. cargo-deny clean with the existing allow-list (no deny.toml changes).
- **limitations/notes:** quota/rate-limit checks (rule 4) deferred to M2 with the quota engine — gateway enforces key validity/expiry only; `cost_micros` pinned 0 (pricing table D23 is M2+); streaming usage fallback counts content chunks (prompt=0) when an upstream ignores `stream_options.include_usage` (OpenAI supports it — fallback is for OpenAI-dialect oddballs); metering-insert failure after a streamed response can only be logged (response already sent) — AUDIT A-003; Windows-local tests use in-memory SQLite because sqlx-0.9 `Any` cannot express absolute Windows paths through its URL form (drive colon parses as port separator) — CI/Linux uses real tempfiles — AUDIT A-002; local dev via Windows-cargo interop still needs `WSLENV=CARGO_INCREMENTAL CARGO_INCREMENTAL=0`; DB queries live in `crates/gateway/src/db.rs` as fully portable SQL + a mechanical `?`→`$n` PG rewrite (sqlx Any does not unify placeholders) — revisit placement when the M2 control plane shares queries; OpenAI-SDK contract tests in CI activate at M5 (director ruling, SPEC §14 M5 row); SQLite connections pin `journal_mode=WAL` + `busy_timeout=5000` via per-connection `after_connect` with a concurrent-writers regression test (director ruling 2026-10-16, M1 follow-up edd59cd) — M5 owns load testing + deeper tuning only (synchronous mode, checkpointing under sustained write load).

### M0 — monorepo scaffold + CI skeleton (2026-10-14)
- **changed:** `Cargo.toml`, `Cargo.lock`, `crates/{gateway,control,ledger,crypto,common}/`, `pnpm-workspace.yaml`, `package.json`, `pnpm-lock.yaml`, `apps/dashboard/`, `.github/workflows/ci.yml`, `deny.toml`, `.gitignore`, `LICENSE`, `README.md`
- **tests:** local — `cargo fmt --all -- --check` ✓, `cargo clippy --workspace --all-targets -- -D warnings` ✓, `cargo test --workspace` 5/5 ✓, `pnpm install` + `pnpm -C apps/dashboard build` ✓, `tsc --noEmit` ✓. CI — [run 38572086285](https://github.com/Phat1911/Tributary/actions/runs/38572086285) green on `main`: rust ✓, dashboard ✓, cargo-deny ✓.
- **evidence:** SPEC §14 M0 "done when" = *PR pipeline green on empty hello-world crates* — pipeline green at `b000c42` on push to `main` (same workflow runs on PRs).
- **dependencies (AGENTS workflow step 10):** Rust — **none** (zero external crates). JS — `next@^16.4.0`, `react`/`react-dom@^19.3.0`, `typescript@^7.0.2`, `@types/{node,react,react-dom}`: the minimal Next.js + TS app M0 requires; **no** Tailwind/shadcn (kickoff decision — arrive with M3 dashboards). pnpm pinned `10.15.1` via `packageManager`. cargo-deny license allow-list = permissive SPDX set + this repo's own `FSL-1.1-ALv2`.
- **limitations/notes:** crates are placeholder libs (`crate_name()` + one unit test each); crate names carry the `tributary-` prefix ahead of crates.io reservation (D19); dashboard is a TS-only placeholder page; `next build` auto-adjusted `tsconfig.json` (`jsx: react-jsx`, `.next/dev/types` include) — kept; local Rust builds on this machine run via Windows-cargo WSL interop (needs `WSLENV=CARGO_INCREMENTAL CARGO_INCREMENTAL=0`), or just use CI.

## Fast jump — most recent milestone → changed files

**M2 (2026-10-17):**

- Migrations: `migrations/0003_orgs_users_sessions.sql` · `migrations/0004_pools_provider_keys_quota.sql` · `migrations/0005_usage_events_pool.sql`
- Common (new home of shared adapter): `crates/common/Cargo.toml` · `crates/common/src/{lib.rs,db.rs,keys.rs,time.rs}`
- Crypto (new): `crates/crypto/Cargo.toml` · `crates/crypto/src/lib.rs`
- Control (new): `crates/control/Cargo.toml` · `crates/control/src/{lib.rs,error.rs,auth.rs,db.rs,api.rs}` · `crates/control/tests/{harness/mod.rs,api.rs}`
- Gateway (M2 rewrite): `crates/gateway/Cargo.toml` · `crates/gateway/src/{main.rs,lib.rs,config.rs,state.rs,error.rs,keys.rs,db.rs,quota.rs,routing.rs,chat.rs,models_api.rs}` (sse.rs unchanged) · `crates/gateway/tests/{common/mod.rs,api.rs,streaming.rs,concurrency.rs,m2_isolation.rs}`
- Docs/conventions: `AGENTS.md` · `SPEC.md` (§8 crypto seam + D30) · `Cargo.lock`

**M1 (2026-10-15):**

- Migrations: `migrations/0001_pool_keys.sql` · `migrations/0002_usage_events.sql`
- Gateway: `crates/gateway/Cargo.toml` · `crates/gateway/src/{main.rs,lib.rs,config.rs,state.rs,error.rs,keys.rs,db.rs,sse.rs,chat.rs,models_api.rs}` · `crates/gateway/tests/{common/mod.rs,api.rs,streaming.rs,concurrency.rs}`
- CI: `.github/workflows/ci.yml` (postgres:16 service + `TRIBUTARY_TEST_DATABASE_URL`)
- Scripts: `scripts/mock_openai.py`
- Lockfile: `Cargo.lock`

**M0 (2026-10-14):**

- Root: `Cargo.toml` · `Cargo.lock` · `pnpm-workspace.yaml` · `package.json` · `pnpm-lock.yaml` · `deny.toml` · `.gitignore` · `LICENSE` · `README.md`
- Rust: `crates/gateway/{Cargo.toml,src/lib.rs}` — and the same pair under `crates/control/`, `crates/ledger/`, `crates/crypto/`, `crates/common/`
- Web: `apps/dashboard/{package.json,tsconfig.json,next.config.ts,next-env.d.ts}` · `apps/dashboard/app/{layout.tsx,page.tsx,globals.css}`
- CI: `.github/workflows/ci.yml`
