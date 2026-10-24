# Contributing to Tributary

Thanks for helping build the pool. Tributary is a Rust AI token-pool
gateway (one binary, one database) with a Next.js dashboard — and it is
written to be *taught from*, so conventions matter here more than in most
repos.

## Read AGENTS.md first

**[`AGENTS.md`](AGENTS.md) is the conventions source of truth** — coding
style, testing, and the project's non-negotiable rules. The highlights
that trip up new contributors:

- **Teaching comments** — the founder is learning Rust, so every
  non-trivial function carries a `///` doc comment (purpose + why), and
  every attribute/derive macro gets a one-line `//` explanation on first
  use per file. AGENTS.md has the exact convention.
- **Dual-DB rule** — every query works on **both SQLite and Postgres**
  (sqlx `Any`); migrations run against both in CI. No DB-specific SQL
  outside the `common` adapters.
- **Money is integers** — micro-USD integer units ONLY. A float used for
  money is a security finding, not a style nit.
- **Secrets hygiene** — never log/trace/print key material; display
  prefix + last-4 only; decrypt per-use wrapped in `Zeroizing`; never
  cache plaintext keys.

## Dev setup

- **Rust** (stable toolchain) — builds the workspace and the single
  `tributary` binary.
- **pnpm 10.x** (pinned `10.15.1` via `packageManager`) + Node 22 — the
  dashboard in `apps/dashboard`.

```sh
# Rust — all three must pass before you push
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Dashboard
pnpm install
pnpm -C apps/dashboard lint
pnpm -C apps/dashboard typecheck
pnpm -C apps/dashboard build
```

## Dual-DB testing

Tests **always** run the SQLite half. To also run the Postgres half:

```sh
export TRIBUTARY_TEST_DATABASE_URL=postgres://user:pass@localhost:5432/tributary_test
cargo test --workspace
```

**Hard rule: a set-but-unreachable `TRIBUTARY_TEST_DATABASE_URL` must
FAIL the tests, never silently skip them** — the test harnesses panic on
a dead Postgres URL by design (the M4 A-006 lesson: a silent skip once
passed a SQLite-only suite as "dual-DB green" and shipped a Postgres-only
bug). If Postgres is genuinely optional for your local loop, unset the
variable; if it's set, it must connect.

Locally, a throwaway Postgres works fine:

```sh
docker run --rm -d --name tributary-pg-test -p 5433:5432 \
  -e POSTGRES_USER=tributary -e POSTGRES_PASSWORD=tributary \
  -e POSTGRES_DB=tributary_test postgres:16
export TRIBUTARY_TEST_DATABASE_URL=postgres://tributary:tributary@localhost:5433/tributary_test
```

## Good-first-issue policy

Issues labeled [`good-first-issue`](https://github.com/Phat1911/Tributary/labels)
are **mentor-guaranteed**: a maintainer will respond within a week —
review, questions, and follow-ups included. Take one, comment `/take` or
just say you're on it, and ask early; we'd rather unblock you on day one
than review a large guess on day seven.

- **No CLA.** Contributions land under the repository's existing license,
  FSL-1.1-ALv2 (Functional Source License 1.1 with an Apache-2.0 future
  license).
- **DCO-style sign-off is not required**, but **conventional commits
  are**: `feat:`, `fix:`, `chore:`, one logical change per commit.

## Security issues

**Never in the issue tracker.** See [`SECURITY.md`](SECURITY.md) — report
privately to tdhphat19@gmail.com. A public issue containing a live
vulnerability endangers every self-hosted deployment.

## Pull requests

1. Fork / branch, commit conventionally.
2. Keep PRs one logical change; run the full check set above first —
   CI (`.github/workflows/ci.yml`) runs the same gates.
3. New/changed dependencies must be justified in the PR description
   (supply-chain discipline; `cargo-deny` runs in CI).
4. Never weaken a test or assertion to make CI pass — investigate
   instead; that's a project rule, not advice.
