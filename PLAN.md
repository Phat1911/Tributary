# PLAN.md — Milestone status & log

> Working document. Status lives here; the permanent tracker with checkboxes lives in SPEC.md §14.

## Current state

- **Date:** 2026-10-14 · **Spec:** v0.3 (decisions D1–D27)
- **Next milestone:** M1 — gateway core (OpenAI proxy, pool keys, streaming, metering)
- **Code:** M0 scaffold complete — cargo workspace (5 lib crates), pnpm workspace (Next.js + TS dashboard), CI green on `main`

## Milestone status

| M | Status | Notes |
|---|---|---|
| M0 | **done** (2026-10-14) | CI green: [run 38572086285](https://github.com/Phat1911/Tributary/actions/runs/38572086285) |
| M1–M6 | pending | V1 — Gateway + Org Pools |
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

### M0 — monorepo scaffold + CI skeleton (2026-10-14)
- **changed:** `Cargo.toml`, `Cargo.lock`, `crates/{gateway,control,ledger,crypto,common}/`, `pnpm-workspace.yaml`, `package.json`, `pnpm-lock.yaml`, `apps/dashboard/`, `.github/workflows/ci.yml`, `deny.toml`, `.gitignore`, `LICENSE`, `README.md`
- **tests:** local — `cargo fmt --all -- --check` ✓, `cargo clippy --workspace --all-targets -- -D warnings` ✓, `cargo test --workspace` 5/5 ✓, `pnpm install` + `pnpm -C apps/dashboard build` ✓, `tsc --noEmit` ✓. CI — [run 38572086285](https://github.com/Phat1911/Tributary/actions/runs/38572086285) green on `main`: rust ✓, dashboard ✓, cargo-deny ✓.
- **evidence:** SPEC §14 M0 "done when" = *PR pipeline green on empty hello-world crates* — pipeline green at `b000c42` on push to `main` (same workflow runs on PRs).
- **dependencies (AGENTS rule 10):** Rust — **none** (zero external crates). JS — `next@^16.4.0`, `react`/`react-dom@^19.3.0`, `typescript@^7.0.2`, `@types/{node,react,react-dom}`: the minimal Next.js + TS app M0 requires; **no** Tailwind/shadcn (kickoff decision — arrive with M3 dashboards). pnpm pinned `10.15.1` via `packageManager`. cargo-deny license allow-list = permissive SPDX set + this repo's own `FSL-1.1-ALv2`.
- **limitations/notes:** crates are placeholder libs (`crate_name()` + one unit test each); crate names carry the `tributary-` prefix ahead of crates.io reservation (D19); dashboard is a TS-only placeholder page; `next build` auto-adjusted `tsconfig.json` (`jsx: react-jsx`, `.next/dev/types` include) — kept; local Rust builds on this machine run via Windows-cargo WSL interop (needs `WSLENV=CARGO_INCREMENTAL CARGO_INCREMENTAL=0`), or just use CI.

## Fast jump — most recent milestone → changed files

**M0 (2026-10-14):**

- Root: `Cargo.toml` · `Cargo.lock` · `pnpm-workspace.yaml` · `package.json` · `pnpm-lock.yaml` · `deny.toml` · `.gitignore` · `LICENSE` · `README.md`
- Rust: `crates/gateway/{Cargo.toml,src/lib.rs}` — and the same pair under `crates/control/`, `crates/ledger/`, `crates/crypto/`, `crates/common/`
- Web: `apps/dashboard/{package.json,tsconfig.json,next.config.ts,next-env.d.ts}` · `apps/dashboard/app/{layout.tsx,page.tsx,globals.css}`
- CI: `.github/workflows/ci.yml`
