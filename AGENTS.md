# AGENTS.md — Tributary working agreement

Read this before touching code. Sources of truth: **SPEC.md** (product & design),
this file (how we work), **PLAN.md** (status & log), **AUDIT.md** (security findings).

## Project structure (target monorepo)

```
├── crates/
│   ├── gateway/        # data plane: axum OpenAI-compatible proxy, metering
│   ├── control/        # control plane: REST admin API, auth, sessions
│   ├── ledger/         # LedgerTrait, DB ledger, chart of accounts (M7+)
│   ├── crypto/         # envelope encryption, key vault (libsodium/RustCrypto)
│   └── common/         # shared types, errors, config
├── apps/dashboard/     # Next.js + Tailwind + shadcn/ui (pnpm workspace)
├── programs/pool/      # Solana Anchor program (empty until M8)
├── migrations/         # sqlx migrations — must run on SQLite AND Postgres
├── scripts/            # price-table import, demo seed, git bootstrap
├── .github/workflows/  # CI/CD
└── deploy/             # Dockerfile, compose files, demo instance config
```

## Coding & testing conventions

- **Rust:** `cargo fmt` clean; `clippy -D warnings`; no `unsafe` without an AUDIT.md entry.
- **DB:** sqlx only; every query works on **both SQLite and Postgres**; migrations run against both in CI; no DB-specific SQL outside `common` adapters.
- **Money:** integer micro-units (micro-USD) ONLY. A float used for money is a security finding.
- **Secrets:** never log/trace/print key material; display prefix + last-4 only; decrypt per-use via `crypto` crate wrapped in `Zeroizing`; never cache plaintext keys.
- **Ledger:** append-only; entries created only through posting-rule templates; balances are caches (drift ⇒ failing test + AUDIT.md entry).
- **API contract:** the OpenAI-compatible surface is a contract — CI tests it with the official `openai` Python and JS SDKs.
- **Tests:** unit tests next to code; integration under `tests/`; property tests for ledger invariants. A milestone is done only when its SPEC "done when" criteria pass.
- **TypeScript:** pnpm; `next lint`; `tsc --noEmit`; no committed `any`.
- **Commits:** conventional (`feat:`, `fix:`, `chore:`), one logical change each.

## Rules — security & economic constraints (from SPEC; non-negotiable)

**Security**
1. Path B only — deposits are API credits/keys; NEVER subscription logins or browser automation of consumer plans (SPEC §4.1).
2. Provider keys: envelope-encrypted at rest; master key from env/KMS, NEVER in the DB (§8).
3. Ledger is append-only, enforced at DB level (no UPDATE/DELETE grants on `ledger_entries`) (§6, M7).
4. Per-user keys; quota + rate-limit checks ALWAYS run at the gateway before any upstream call (§5, §9).
5. Canary detection stays in the free tier; response automation is paid. Free-vs-paid is a packaging split (public core vs licensed build), never a runtime flag (D17).
6. Never weaken a test or assertion to make CI pass — investigate instead.

**Economic**
7. Prepaid renter balances are LIABILITIES, never revenue. Nightly solvency check: `cash:escrow ≥ Σ payable + Σ prepaid` — a violation is a High finding (§11).
8. Operator fee stays within the 5–10% band (target 8%, D21). Changing it requires explicit user approval + a D-log entry.
9. No token sale/ICO — shares are ledger claims only (§4.4).
10. Chain-optional UX: members/renters never need a wallet; chain settlement is back-end only (§4.5).

## Default workflow

1. Read `AGENTS.md`, `SPEC.md`, `PLAN.md`, and the current worktree before changing code.
2. Select only the next incomplete milestone; restate its scope and required verification (the SPEC "done when").
3. Implement the smallest coherent change — no unrelated refactoring, no silent specification changes (spec changes need explicit user approval and a SPEC edit in the same commit).
4. Format, compile, and run the milestone's required tests. Investigate failures instead of weakening assertions.
5. Update `PLAN.md` status + milestone log, and `AUDIT.md` with evidence for any security findings discovered while implementing, testing, and auditing.
6. Tick the milestone checkbox in `SPEC.md` §14 tracker and append the implementation location (`— implemented at <commit-shorthand> <paths>, <date>`).
7. Do not combine or skip core milestones unless the user explicitly asks. Stop and ask when an implementation choice would alter an approved rule (Rules above or SPEC §4).
8. Report changed files, test results, and known limitations. End every milestone report with a **fast jump** list — exact paths touched this milestone.
9. *(Added)* New spec ambiguity discovered mid-work: record it in `AUDIT.md`; continue only if non-blocking, otherwise stop and ask.
10. *(Added)* Any new/changed dependency must be justified in the `PLAN.md` entry (supply-chain discipline; `cargo-deny` runs in CI).
11. *(Added)* **Parallelism policy:** sub-agents only for tasks that are large, have disjoint write-scopes, and share an interface that is fully agreed *before* the split. Scaffold/shared-config work and interface definition stay sequential. The milestone kickoff prompt names where parallelism is allowed.

## Session protocol (director–worker model)

- Milestones execute in **dedicated worker sessions** (fresh context, workspace rooted at the repo). A long-lived **director session** (holds design memory) orchestrates across milestones.
- Before each milestone the director scans state (SPEC tracker, PLAN, AUDIT, git), recommends *new session vs. continue*, and issues a **kickoff prompt** the user pastes into the worker session.
- Worker sessions follow the Default workflow. Spec-change requests and rule questions route back through the user to the director.
- On completion the worker updates PLAN/AUDIT/SPEC; the user returns to the director, which **verifies** the files and runs a short retro before issuing the next kickoff.
- There is no cross-session telepathy: **the files are the only interface.** If it isn't in the files, it didn't happen.
