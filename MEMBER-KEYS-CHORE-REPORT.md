# CHORE report to director — member keys table: surface expiry + cap

**Kickoff:** director mini-kickoff 2026-10-22 (chore-grade, pre-M5), GO
with four rulings. **Worker session close:** same day.
**Commits:** `4d26579` (chore) → `e0fda1b` (version-skew fix from the
founder's first live run) → `e4394c3` (PLAN record of the fix).
**Status:** closed. No milestone ticked (M5 owns the tracker). PLAN.md
carries the full chore log entry + fast-jump; this file is the
worker→director hand-off summary.

## Scope executed (vs. kickoff)

1. **API** — `GET /me/keys` payload extended with `expires_at` and
   `cap {limit_tokens, period, used_tokens}`: fields that already existed
   in the M4 schema (`pool_keys.expires_at`, `quota_rules.limit_tokens`,
   `usage_events`). Masked key only; nothing else changed; read-only — no
   new enforcement, schema, cron, or second quota concept.
2. **UI** — two columns after Issued on `/me`: **Expires**
   (`formatDateTime` matching Issued, `Never` for null, amber tint < 7 d,
   display-level `expired` chip — Status column keeps DB truth; no
   lifecycle state added) and **Cap** (`used / limit · window` in the unit
   M4 ships = **tokens**, e.g. `12.4k / 100k · day`; `Unlimited` when no
   token rule binds a usable key; `—` for revoked/rotated; suspended keys
   keep their cap — restorable).
3. **Constraints** — rules 1–10 upheld (no key material beyond the
   existing masked prefix; no money floats; no spec change); teaching
   comments throughout; dual-DB proof for the changed/added queries;
   eslint + tsc + build clean.
4. **Proof** — step-12 journey through the public API only + founder
   manual journey doc (`MEMBER-KEYS-FOUNDER-TEST.md`, K1–K6).

## The four rulings → where they landed

| Ruling | Implementation |
|---|---|
| min-headroom across key/user/pool | `db.rs::effective_cap` (pure, unit-tested): among all token rules binding the key through its three subjects, the smallest `limit − used` wins — the rule that would deny first |
| `used` = current-window tokens, mirroring enforcement | `db.rs::tokens_used_since` — same SUM/column mapping/window math as `quota.rs::usage_tokens`, incl. the CAST-to-BIGINT Any-driver rule; period fallback to `day` identical to the engine's |
| visible muted period suffix, not tooltip | `CapCell` renders `12.4k / 100k · day` (`formatTokensCompact` — new, drops trailing `.0`; `formatTokens` itself untouched so existing surfaces keep their output) |
| commit stages explicit paths only | three commits, each `git add <paths>`; director's dirty `.gitignore`/`AGENTS.md`/`SPEC.md` left dirty throughout |

## Findings worth director eyes

- **React-19 lint forced a clock redesign.** `Date.now()` in render trips
  `react-hooks/purity`; the classic setState-in-effect fix trips
  `react-hooks/set-state-in-effect`. Resolved with `useWallClock`: the
  client clock as a `useSyncExternalStore` external store — module-cached
  snapshot refreshed by a 60 s tick, constant server snapshot so the
  tint/chip can never hydration-mismatch. No rule weakened; no new deps.
- **Version-skew crash (founder's live run, fixed `e0fda1b`).** A
  pre-chore `serve` binary sends no `expires_at` → field arrives
  `undefined`, not `null` → `RangeError: Invalid time value`. Both cells
  now treat absent-as-`Never`/`Unlimited`; the may-be-absent contract is
  documented on the TS interface. **M5 packaging note:** dashboard-newer-
  than-backend is the normal upgrade path — optional API fields must
  degrade in the UI, never crash. Not tested automatically (no UI test
  infra until Playwright at M5).
- **`limit_cost` stored-but-unenforced:** already recorded — PLAN M1
  notes, SPEC §5/D23, and the bind-site comment in `api.rs` — so per the
  director note, **no new AUDIT line** was added.
- **PLAN.md "Current state" header is stale** (still "next: M4", dated
  2026-10-17). Director housekeeping — left untouched (unrelated edit).

## Evidence

- `cargo fmt --all -- --check` ✓ · `clippy --workspace --all-targets --
  -D warnings` ✓ · `cargo test --workspace` **132/132 dual-DB** (SQLite +
  Postgres `trib_test_ci` :5433; was 127). New: 4 resolver unit tests +
  `self_service::member_key_list_surfaces_expiry_and_effective_cap`
  (self_service 4→5) — provisions org/pools/members/keys-with-expiry/
  quota-rules entirely through the public API; only metering rows seeded
  (gateway owns their sole write path — the sanctioned step-12 exception).
  Journey proves: exact window usage (12.4k counted, a pre-window 50k row
  excluded), cross-subject resolution (user-level 50k/2k beats pool-level
  1M/month), rate-only rule surfaces no cap, expired key carries
  `expires_at` while status stays `active`, no raw-key or cross-member
  leakage.
- Dashboard: `eslint` ✓ · `tsc --noEmit` ✓ · `next build` ✓ (11 routes).
- No new dependencies (workflow step 10 n/a — everything hand-rolled).

## Environment incidents (not code; disclosed for the retro)

1. Windows `link.exe` LNK1000 flake linking over the UNC (`\\wsl.localhost`)
   target; plus a stale `tributary-gateway.exe serve` (PID 17948) locking
   `target\debug` — killed it (test leftover). The founder's later serve
   (PID 4560) was left RUNNING untouched; the verified suite therefore ran
   in an isolated `CARGO_TARGET_DIR=C:\Users\HONG PHAT\tributary-chore-target`.
2. One transient Postgres connect refusal under the parallel connect
   storm — the harness's anti-skip PANIC fired exactly as designed (M4
   final-scan hardening working); clean on rerun.
3. The founder must rebuild+restart `serve` onto `4d26579+` to see live
   data (their pre-chore binary was the version-skew trigger above).

## Fast jump — exact paths

- Control: `crates/control/src/db.rs` · `crates/control/src/me.rs` ·
  `crates/control/tests/self_service.rs`
- Dashboard: `apps/dashboard/app/me/page.tsx` · `apps/dashboard/lib/api.ts`
  · `apps/dashboard/lib/format.ts`
- Docs: `MEMBER-KEYS-FOUNDER-TEST.md` · `PLAN.md` (chore entry + fast-jump)
  · this file
