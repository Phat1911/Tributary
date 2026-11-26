# PLAN.md — Milestone status & log

> Working document. Status lives here; the permanent tracker with checkboxes lives in SPEC.md §14.

## Current state

- **Date:** 2026-11-26 · **Spec:** v0.3 (decisions D1–D44)
- **M11-A: DOCTRINE SUITE ON BRANCH `m11/doctrine-suite` — EXPECTED-RED, GO gate pending (2026-11-26).** Session A (doctrine-suite mode, step 14) shipped the stub contract and the ten-test suite; **no product logic — every settlement handler and all four Merkle helpers are `todo!()`**. `programs/pool/src/settlement.rs` (declarations: seeds, `SettlementRoot`/`ClaimMarker`/`OperatorBond`, six typed account lists, six events, the Merkle recipe), six `todo!()` handlers in `lib.rs`'s dispatcher (`fund_bond`, `post_root`, `claim`, `challenge`, `resolve_challenge`, `slash_bond`), seven `PoolError` codes appended last (6012–6018), and `programs/pool/tests/m11_doctrine.rs` — **ten tests named as the ten sentences, 0 passed / 10 failed in CI for the right reason** (each at a must-land step the stub cannot satisfy). **CI verdict of the suite head `4ec3d42`: [run 45773107548](https://github.com/Phat1911/Tributary/actions/runs/45773107548) — red on the Rust job ONLY because of `m11_doctrine`; SBF build, fmt, clippy (both legs) and every other test binary in that job green (pool 9 unit + 20 adversarial + 9 audit_session_c + 4 journey, the whole non-gateway workspace); Dashboard ✓, Dashboard E2E ✓, OpenAI SDK contract ✓, Supply chain ✓.** **Two findings for the gate: A-058** — the Anchor cohort exposes NO hash primitive, so Session B must hand-write SHA-256 (recommended), declare the `sol_sha256` syscall (needs `unsafe` + an AUDIT entry), or get a dependency approved; **A-059** — the M8 mirror becomes `vault == Σ prepaid − Σ payouts` once claims pay. The founder and director read the test names first; Session B never edits a test. Full entry in the log below.
- **CI-GREEN CHORE — DONE (2026-11-26): main's two reds cleared, no product code.** (1) cargo-deny RUSTSEC-2026-0592 — `url` 2.5.8 → 2.5.9 with its FORCED companion `form_urlencoded` 1.2.2 → 1.2.3 (the only two lock entries; the companion is required by `url` 2.5.9's own manifest — the kickoff's tripwire fired and the director ruled "land both"); `deny.toml` untouched; A-057. (2) The flaky `a_refused_pick_falls_back_to_sibling_capacity` pin — diagnosis CONFIRMED (the hunt drained B too: 19% per DB leg), setup fixed by depositing B AFTER the hunt, all five assertions byte-identical, 50/50 binary runs green per database. Local gates: fmt / clippy / deny clean, full workspace dual-DB 198/198, standing pins 2 green / 10 red. **CI verdict of the pushed fix head `3a29811`: GREEN — [run 45750240229](https://github.com/Phat1911/Tributary/actions/runs/45750240229), all five jobs.** Full entry in the log below.
- **M9-D: FIX SESSION — Session D DONE (2026-11-19, persona: senior payment-systems engineer; fixes never weaken tests): ALL SEVEN Session C findings closed (A-047..A-053 fixed, A-054 addressed) — the 10 exploit PoCs in `crates/gateway/tests/m9c_audit_poc.rs` are RED forever (exploits dead; the file EDIT-FROZEN per the kickoff, no ruling named its tests), the 2 clean probes stay GREEN, and CI now enforces that standing-pin state as a first-class invariant (the "Standing pins" step fails unless exactly 10 red + 2 green). Headline fixes: settle-side clamp with operator-eats-it overage doctrine (A-047), settle-run journal + delta attribution + identical-window replay (A-048), /settle POSTS THE BOOKS via the founder-approved NEW template `capacity.consume` (Dr claims.contributed / Cr revenue.usage) → settle.fee → settle.distribute, positions become true caches with claims/capacity mirror invariants (A-049, per the director ruling + founder approval 2026-11-19), serves-based bucket admission + smallest-name/provider tie-breaks (A-050), status filter in resolution (A-051), REAL fallback on refused picks (A-052, founder-ruled in-doctrine — no suite edit proved necessary), saturating metering + i128 settlement + zero-report estimate floor (A-053). Permanent successor pins live in `crates/gateway/tests/m9d_fix_pins.rs` (8 tests, additive). cargo-deny: humantime/chrono/lettre advisories ignored-with-justification per founder ruling (A-055/A-056; revisit tripwire 2026-11-24 after the hackathon). Full entry in the log below.
- **M9-C: ADVERSARIAL AUDIT — Session C DONE (2026-11-18, systems-accounting attacker persona): 8 findings (3 high / 3 med / 1 low + suite-recursion low), all PoC-proven dual-DB in `crates/gateway/tests/m9c_audit_poc.rs` (12/12 assert-the-exploit green on the vulnerable tree — each flips red the day its finding is fixed). Findings live in AUDIT.md A-047..A-054 with the founder's four mandatory pointers; headline: per-bucket overdraw IS possible (estimate-gap, A-047), settle windows double-credit (A-048), /settle never posts the books so rule-7 solvency is blind to payables (A-049), and bucket resolution is a free-string ordering game — a 1%-weight deposit captures 100% of a model's traffic (A-050). Bait verdicts: #1 verified (A-048), #2 verified (A-052), #3 refuted for the hot path (single-arg stubs are caller-less; `_for` forms carry multi-bucket keys), #4 verified two ways (A-053). Attempted-clean categories + full report in the session close-out; no code or existing test was touched. Findings route to a fix session through the user. Full entry in the log below.
- **v0.1.4 "TRUTH RELEASE" SHIPPED (2026-11-11):** docs-honesty release closing the external review's findings — README business-model/truth restructure, storefront repo description+topics fixed, deploy-docs truth pass, `audit_log` DB-enforced append-only (A-046), threat-model doc, `create-admin` env password; tag `v0.1.4` on `cc9f479` (CI green, E2E flake re-run), GHCR `0.1.4`/`latest` multi-arch, release notes WITH the depositor-experimental + audit-this-week sentences, demo 8112 re-pinned (healthz 200). Full entry + fast jump in the log below. M9's Session C (adversarial audit) remains the next gate.
- **M9-B: IMPLEMENT TO GREEN — Session B DONE on `m9/doctrine-suite`: 13/13 red → 0, dual-DB; merged to main (merge SHA + main-CI verdict recorded at close-out below).** Depositor mode implemented to the Session-A contract without touching a single test: the D43 split (`settle::split_settlement` — earnings follow the burn, per bucket, deterministic largest-remainder micros), the routing policy (`ProRataByClaims` — weight IS claims.contributed; splitmix64 draw mixing), the bucket/key registries (hyphen-prefix longest-match families, revocation-aware eligibility), the per-bucket overdraw guard (single conditional UPDATE, A-020 shape, fail-closed), burn attribution at settle-time only, the M9 pricing slice v0, and the three public endpoints (`POST /pools/{id}/deposits`, `GET /pools/{id}/bucket-positions`, `POST /pools/{id}/settle`) plus `kind:"root"` on pool creation. **Pricing v0 note (director GO scope note 1): `PRICE_MICROS_PER_TOKEN = 150` µ$/token (=$0.15/1K tokens) in `crates/gateway/src/depositor_mode.rs` — a PLACEHOLDER with a named owner (D23's price table replaces it); scoped ROOT-pools-only because V1 pins `cost_micros == 0` (gateway tests/api.rs "cost stays 0 until pricing").** **Settle asymmetry (director GO scope note 2, flagged to Session C): `/settle` updates `depositor_bucket_positions.earned_micros` directly and does NOT post `settle.fee`/`settle.distribute` through the books (asymmetric with `/deposits`, which posts `deposit.capacity`); the ledger side of settle stays pinned by the ledger suite via the public trait.** Session C (adversarial audit) is the next gate. See the milestone log entry below.
- **M9-A: DOCTRINE SUITE ON BRANCH `m9/doctrine-suite` — expected-red artifact SUPERSEDED by M9-B (suite now green).** Session A (doctrine-suite mode, step 14) filed D43 + A-044 same-commit with the suite: migration 0016 (attribution columns + `depositor_bucket_positions`, additive only), stub interfaces (`crates/ledger/src/settle.rs`, `crates/gateway/src/depositor_mode.rs` — the A/B contract, all `todo!()`), and the 14-test doctrine suite (`crates/{ledger,gateway}/tests/m9_doctrine.rs`) whose names ARE the 11 kickoff sentences. Invariant 9's family-mode pin is GREEN on the branch by design (it pins UNCHANGED V1); every behavioral M9 test was red on the stubs. GO granted 2026-11-11 with two refinements (recorded in the M9-B log entry). See the milestone log entry below.
- **M7: DONE (2026-10-28) — V2's truth spine, shipped dark.** LedgerTrait + double-entry ledger at f0c25ca→8bbee09: migration 0015 (chart/accounts/entries/balances-cache, dual-DB), DB-level append-only on both engines (SQLite RAISE(ABORT) triggers re-armed every start; PG `tributary_app` role split, SELECT+INSERT only) with red-first burglar alarms, posting-rule-only creation (6 templates), property tests (per-txn balance, trial balance, normal balances, cache reconstruction, rule-7 solvency never violable by template flows), loud alarms (drift/trial-balance/solvency), journey proof via public trait only, deploy/README honest-boundary section. D38 recorded same-commit. **Zero V1 behavior change: full workspace 172/172 dual-DB, fmt/clippy/geoip-clippy clean, zero new dependencies.** Kickoff's "next: 0005" was stale — director verified, used 0015. Director chart rulings (rule-7 membership + fee placement) recorded in D38/AUDIT A-030/031. Known limitation: the binary still connects as DB owner; the app-role flip is an M10 deployment step (role exists, CI-tested). Next: M8 (Solana program — shares, escrow).
- **M6: IN PROGRESS — slice 1 DONE (2026-10-27, docs) + M6-S1-F1 FIXED & released as v0.1.3 (same day, director extension).** Pilot onboarding surface shipped against live v0.1.2: pilot-feedback issue template, deploy/README upgrade rung (pull REQUIRED, tag-pin, rollback verified 0.1.2→0.1.0→0.1.2), docs/pilot-support.md runbook, docs/onboarding-org-admin.md DRAFT v1 (empty box → first request, Playwright-verified green incl. pre-upstream quota proof). Slice-1 finding M6-S1-F1 (minute-budget dialog sent `limit_tokens` labeled "(requests)" — bricked members) fixed red-first with a CI-enforced wire-payload pin (new Playwright e2e job, first run green), released as **v0.1.3** (run 40507195511), smoke-verified on the released image, demo 8112 re-pinned; docs reworded with pre-0.1.3 symptom retained. Slice 2 (guide rewrite) waits on founder pilot-1 friction log. M6 done-when unchanged: first real org daily-driving it.
- **M5.5: DONE (closed 2026-10-27, slice 2 = Gemini)** — slice 1 (2026-10-26): landing live at https://phat1911.github.io/Tributary/, Tailscale rung stranger-proofed (findings #1–#7), video script, journeys A/B/C PASS. Slice 2 (2026-10-27): Gemini as a first-class pool provider — proven provider-agnostic by construction (no gateway-code change; base_url + Bearer already spoke the compat endpoint). J-A/B/C public-API journeys dual-DB green (`m55_gemini.rs`); J-D official-SDK contract legs vs mock Gemini-compat upstream green (same CI script); dashboard provider select + column; README/deploy/landing honest paragraphs + model-ID verify curl + rate-limit honesty; J-E founder real-key script shipped (`docs/founder-journey-m55-slice2.md`). Demo stack RE-STAGED collision-proof (`tributary-demo` @8112, dual-homed on demo-net, stories green, shadow flag dismissed, usage parity re-seeded: 281 rows / 15 days / 460K tokens). **Residue until real: demo video records ~2026-11-12, README link lands same day — Nov-7 director checkpoint chases it.** Next: M6 (pilot onboarding; adoption focus, V1 entirely free per D35).
- **Code:** M5 complete and director-verified (2026-10-25): full hardening block red-first (A-019/020/021/022bc/026/028), packaging + GHCR v0.1.0 live, SDK contract tests in CI, public scrub + full-history rewrite clean, founder stranger run PASSED. Close-out 53c0430. Known leftovers: stopwatch number (optional, M5.5), D32 sequential-sharing blind spot (documented), concurrency_flags_for_key cleanup decision (open, pre-M5 leftover).
- **Post-close events (2026-10-25/26):** LinkedIn humble-launch post shipped (four ask doors per D34); launch-post assets chore done (79bbb80 — seed_post_demo.py, post_shots rig, stack staging); stranger-finding #4 fixed live: quickstart master-key step promoted to its own line (969cfdf); GDG hackathon entry recorded (D36); Product Hunt launch planned for early December post-M5.5 (feedback-not-upvotes; vault holds the doctrine).
- **DevExchange artifacts committed (2026-11-11, v0.1.4 chore 0):** `demo/devexchange-slides.html` + `docs/devexchange-{demo,runbook}.md` — the sources behind the already-deployed Pages slides (`b9219c9`) and the frozen Nov-7 event materials; committed verbatim (their v0.1.3 references are historical fact, not current-version claims).

## Milestone status

| M | Status | Notes |
|---|---|---|
| M0 | **done** (2026-10-14) | CI green: [run 38572086285](https://github.com/Phat1911/Tributary/actions/runs/38572086285) |
| M1 | **done** (2026-10-15) | CI green incl. dual-DB tests: [run 38682358525](https://github.com/Phat1911/Tributary/actions/runs/38682358525); local end-to-end smoke vs mock upstream ✓; real-OpenAI curl demo pending user key |
| M2 | **done** (2026-10-17) | CI green on `main` incl. dual-DB tests + cargo-deny: [run 38996743570](https://github.com/Phat1911/Tributary/actions/runs/38996743570); local dual-DB 59/59 (SQLite in-memory + Docker Postgres 16 on :5433); done-when `m2_isolation.rs` green |
| M3 | **done** (2026-10-17) | CI green on `main`: [run 39051018301](https://github.com/Phat1911/Tributary/actions/runs/39051018301) — Rust (fmt/clippy/test dual-DB) ✓, cargo-deny ✓, dashboard lint+typecheck+build ✓ (first run 39050675931 caught a lost script entry — fixed in 8efa119). Local: Rust dual-DB 76/76 twice consecutively (SQLite in-memory + Docker Postgres 16 `tributary-pg-m3` on :5433, removed after) + dev-proxy smoke vs a mock control plane; done-when: org-admin journey through the UI (see log) |
| M4 | **done** (2026-10-22) | Closed at 38caee5 + round-2 fixes (A-023…A-029); two adversarial waves, 12 journeys, 127/127 dual-DB at close; open items A-019…A-022 ruled into M5. Post-close chore: member-keys expiry+cap surfacing (314ceb1, 132/132) |
| M5 | **DONE (closed 2026-10-25)** — director D35 descope ruling: docs site + landing + hosted demo → M5.5 (demo = recorded video over private network); D34 stub + D35 + M5.5 + rewritten M6 row swept into close-out. All done-when gates passed (stranger run, XFF-CIDR-pin proof, burst-bounded-by-estimate proof); §14 ticked with full implementation stamp. Was: | Hardening batch 1 committed at f40b99c (A-019 trusted proxies, A-020 quota reserve→settle→release, A-021 D32 dedupe, A-022c key-kind check, A-028 config-epoch + min-shadow) — all red-first dual-DB. A-022b done in batch 2: direct gates 403 approval_required + POST /pending-actions request endpoint (6 kinds) + all 9 legacy 202 asserts re-pinned to the M5 contract with end-state assertions intact (director note 1) + reserve-path 429 message distinguishes estimate-denial from cap-denial with formula inline (director note 2; deploy-docs half lands with Phase 2 packaging) + test IPs moved to transport sockets (A-019 production truth; shared-host bootstrap pattern in m5 harness). Remaining: A-026 new kinds (admin_key_quota, admin_key_quota_delete, admin_key_allowlist_add/remove, admin_key_mint, pool_quota_disable, pool_quota_rule_delete, provider_key_revoke, org_delete) — pattern: SensitiveKind variant + request-endpoint validation arm + execute arm + direct-endpoint gate. **RULING CORRECTION (director, 2026-10-24): provider_key_revoke has NO containment exemption — 4-eyes ALWAYS at ≥2 admins; single-admin orgs auto-pass WITH the audit marker.** Implemented as ruled. **A-026 DONE (batch 3): all 9 kinds live** — SensitiveKind variants + request-endpoint validation arms + execute arms + direct gates (upsert/delete quota-rules, allowlist add/remove, key mint, provider revoke ALWAYS, org delete before content-check), red-first (target/red-proof/a026-red.log, 6 behavioral failures) then 6/6 green dual-DB in m5_a026_gates.rs; fixtures updated to B-self-mint (mint-for-another-admin now correctly gates). **Flake #3 → AUDIT line (as directed): SQLite deferred-tx upgrade race in demote_org_admin_guarded — fixed with BEGIN IMMEDIATE arm, 3/3 stable.** /healthz DONE (72461b7): DB-backed 200/503 on the full app, dual-DB test. Phase 2a REVIEWED + FIXED (findings from live verification): (1) dashboard static export was blocked by the dynamic route /org/pools/[id] — converted to /org/pools/?id= query param + Suspense + trailingSlash, export now green (lint+tsc clean); (2) the binary did not serve the export — added ServeDir fallback via TRIBUTARY_STATIC_DIR (tower-http added to [dependencies], justified per workflow step 10: hand-rolling static serving would re-implement MIME + traversal guards); (3) REAL BUG: sqlx sqlite defaults create_if_missing=false — a fresh /data volume boot-looped ("unable to open database file"); fixed with mode=rwc in the URL connect; (4) REAL BUG: entrypoint.sh mode 711 (unreadable by the runtime user — shell scripts need r) — file normalized to 755 + Dockerfile chmod 0755; (5) deploy docs: quota estimate-formula disclosure added (director note 2), stale phase notes removed, port rationale generalized, rust tag 1.98→1 (local 1.99.1). Docker image builds (110MB) and container verified end-to-end: dashboard 200, /healthz 200, pools page 200, fresh-volume boot. Scrub COMPLETE at f924d05 (D20/D21/D27 de-specified, D29 tail removed; R1 Polar row + R2 audience line ruled KEEP). Phase 2a committed dd71729 (in-container create-admin+login verified). J7 DONE (f7cae37): scripts/contract_openai_sdk.py — official openai Python 3.37 + JS SDKs against the live /v1 (chat.completions + models.list, public-API provisioning via the manual_journey bootstrap), green locally, CI job `sdk-contract` added. J6 DONE (4f73c81): scripts/e2e_j6.py boots the real stack (static export served BY the gateway, exactly like the container) + apps/dashboard/e2e — Chromium walks login → create org → org visible, then an in-page fetch asserts UI and /api/v1/orgs agree (no-skew leg; page.evaluate because the Secure session cookie rides the localhost http:// exception in the browser but not in Playwright's API context). Stranger-run SIMULATION passed (2026-10-24): fresh clone → .env → docker build → boot → healthz 200 → dashboard 200 → create-admin → login 200, total 502s (~8.4 min, warm layer cache; build 493s). Two real defects caught and fixed en route: .env.* ignored deploy/.env.example out of clones (072cce4), and tsc picked up e2e/ inside the image build (071d7f0). CLEAN-machine caveat: a cold Rust build will exceed 10 min — the mitigation is publishing the GHCR image (tag → release.yml) and letting compose prefer the prebuilt image; founder decision at close. PUBLISHED (2026-10-24, director-ruled sequence): root README rewritten as the stranger's first tab (v0.1.0 status, ≤5-line quickstart, honest timing, links row) — e412b2e-equivalent, now a1f4d39-side; compose prefers the prebuilt GHCR image (pull_policy: missing) with source-build fallback. FULL-HISTORY REWRITE (director Option 1): pre-scrub jewels (pricing figures, pilot identities, D29 tail) lived in ~90 public commits — filter-repo replace-text (17-rule table) + M3-FINAL-REPORT.md path-purge, all refs, one pass; verified ZERO matches for every pattern across all 101 rewritten commits, HEAD tree byte-identical, commit count preserved; untouched history vaulted at target/tributary-pre-rewrite.bundle (a00f92d). Force-pushed main (38caee5→a1f4d39), remote SPEC/README verified clean. RECORDED CAVEAT (director, not actioned): GitHub may serve unreachable commits by direct SHA for a while; no forks exist; zero-audience residual ≈ nil. Tag v0.1.0 pushed → release.yml → GHCR image. v0.1.0 SHIPPED: ctutils fix (5e66ba6) re-tagged as v0.1.0; release run 40101073319 green; ghcr.io/phat1911/tributary:{latest,0.1.0} publicly pullable; the patched :0.1.0 image smoke-verified end-to-end (healthz 200, dashboard 200, create-admin, login 200). First post-publish CI also validated the pipeline (cargo-deny caught RUSTSEC-2026-0385 in ctutils 0.4.3 → 0.4.5). FOUNDER PERSONAL RUN PASSED (2026-10-25 08:37 ICT): fresh clone → README-only quickstart → compose pulled ghcr.io/phat1911/tributary:latest → create-admin → logged-in host dashboard at localhost:8111, zero assistance beyond two README findings that are now IN the README (shell requirement caa628b; compose-plugin diagnosis + compose-free fallback a1db959). Wall clock from instructions to report ≈13 min including human pauses; the machine path (pull+boot+admin+login) is the ~2–3 min leg per simulation — exact stopwatch number not captured (re-timing optional: docker compose down -v, rm -rf ~/Tributary, re-run). M5 GATE STATUS: stranger run ✓, hardening proofs ✓ (spoofed-XFF pin + burst/cap, dual-DB), SDK contract ✓ (CI), packaging/GHCR ✓. OPEN before the §14 tick, per the SPEC M5 row's own scope: docs site + landing page (posture line ruled, not built) + hosted demo (D25 Hetzner, not deployed) — tick requires shipping them or an explicit director descope. J1 XFF legs permanently pinned in m5_journeys (spoofed-ignored :391, trusted-honored :473 incl. forged-left-hop + all-trusted-chain) |


## M5 close-out (2026-10-25)

**Gates:** founder personal stranger run PASSED (README-only, pull path, logged-in host dashboard); spoofed-XFF-cannot-pass-CIDR-pin pinned dual-DB (m5_journeys :391/:473); burst-cannot-exceed-cap-by-more-than-one-estimate proven (A-020 tests); OpenAI SDK contract green in CI (official Python+JS); packaging/GHCR live and smoke-verified. Stopwatch number outstanding-for-the-record (2–3 min machine path honest; re-time at M5.5 if the landing page ever carries a number).

**Descope (D35):** V1 entirely free — no paid tier/support/hosting; deployment client-owned (VPS/Tailscale/LAN); D25 demo amended to recorded video over private network; D19 domains deferred; M5.5 row carries landing ("everything free" posture) + demo video + Tailscale rung in deploy docs; M6 = adoption focus, "≥1 pays" void. Tailscale blessed as rung-3; founder tailnet never a production dependency.

**Fast jump — everything M5 touched:**
- Hardening: `crates/{control,gateway}/src/**` (approvals, api, db, quota, auth/lib), `migrations/0012–0014`, tests `m5_journeys.rs` `m5_a026_gates.rs` `m4_journeys.rs` (re-pins) `common/mod.rs`
- Packaging: `deploy/{Dockerfile,entrypoint.sh,.env.example,README.md}` `docker-compose.yml` `.dockerignore` `README.md` `CONTRIBUTING.md` `SECURITY.md` `.github/workflows/{release.yml,ci.yml}` `apps/dashboard/{next.config.ts,package.json,tsconfig.json,app/org/pools/page.tsx,playwright.config.ts,e2e/}`
- Binary additions: `/healthz` + ServeDir static serving (`crates/gateway/src/lib.rs` `config.rs` `db.rs`, `crates/common/src/db.rs` create_if_missing)
- Journeys/tests: `scripts/contract_openai_sdk.py` (J7) `scripts/e2e_j6.py` (J6)
- Records: `SPEC.md` (scrub f924d05 + D34/D35/M5.5/M6 + §14 tick) `PLAN.md` `AUDIT.md` (flake #3) `.gitignore`
| M5.5 | **DONE (closed 2026-10-27)** | Slice 1 (landing/Tailscale/video script) 2026-10-26 + slice 2 (Gemini provider, J-A…E) 2026-10-27; §14 ticked. Residue: video records ~2026-11-12 |
| M6 | **in progress — slice 1 + F1 fix/v0.1.3 DONE** (2026-10-27) | Pilot onboarding surface (docs: feedback template, upgrade rung, support runbook, admin guide DRAFT v1) + M6-S1-F1 fixed red-first, CI-pinned, released v0.1.3, demo re-pinned. Slice 2 waits on founder pilot friction log |
| M7 | **done** (2026-10-28) | LedgerTrait + double-entry ledger, ships dark; f0c25ca→8bbee09; 172/172 dual-DB (16 new tests); D38 recorded; §14 ticked |
| M8 | **done (2026-11-01)** | Devnet soak COMPLETE: all 10 steps green (evidence in docs/devnet-soak-m8.md + deploy/devnet-m8.json); §14 ticked. Session C audit (2026-11-02): A-041/A-042 **fixed** (deployment-authority gate; post-CPI event reloads), A-043 accepted-until-M10 |
| M9–M11 | pending | V2 — Root Pool |
| M12–M14 | pending | V3 — Operator network |

## Milestone log (append per completed milestone; keep the most recent 10)

### 2026-11-26 — M11-A: doctrine suite (Session A, step 14) — EXPECTED-RED on `m11/doctrine-suite`, GO gate pending

**Doctrine-suite mode, second execution (M9-A was the first).** Session A restated the ten sentences, checked them against SPEC §10/§12, D38–D44, A-039, A-041..A-043 and the program, and found no conflict that blocks a test — the two boundary questions became AUDIT rows (A-058, A-059) rather than a STOP. Then, on the one branch: the stub contract and the ten-test suite. **No product logic: every settlement handler body and all four Merkle helpers are `todo!()`.**

**Stub contract (what Session B implements to green, never editing a test):**
- **Six handlers in `lib.rs`'s dispatcher** — `fund_bond(amount)` · `post_root(window, merkle_root, total_payout)` · `claim(window, leaf_index, depositor, amount, proof)` · `challenge(window, evidence)` · `resolve_challenge(window, ruling)` · `slash_bond(window)`. They live in `lib.rs` because anchor-syn 1.2.2 registers only functions written INSIDE `#[program]` and imports the Accounts macros by CRATE-ROOT path (hence `pub use settlement::*;` — a named re-export carries the types but not the macros; the compiler says only "unresolved import `crate`"). Declarations: `programs/pool/src/settlement.rs`.
- **State / PDAs:** `SettlementRoot` `[settlement_root, window_le]` (plain `init` — one per window, EVER) · `ClaimMarker` `[claim_marker, root, leaf_index_le]` (plain `init` — one per leaf, EVER) · `OperatorBond` `[pool, bond]` and its Token-v3 vault `[pool, bond_vault]` (authority = pool PDA). Invariants 3 and 7 are therefore enforced by ADDRESSES; their refusals come from account creation and the suite pins the STATE.
- **Doors:** `claim` is two-door like `burn_claims`/`refund_renter` (backend OR the leaf's depositor; `destination.owner == depositor` is an account constraint; a separate rent `payer` so the direct door needs nothing from the depositor but a signature) · `challenge` is permissionless (SPEC §10) · `resolve_challenge` is multisig-gated (`has_one = admin_authority`) · `slash_bond` is a permissionless crank that takes NO amount and NO destination argument — both come from the root account.
- **Ruling:** `RootStands` | `RootFalse { slash_amount, victim }`. **Status:** `Active → Challenged → Active | ProvenFalse → Slashed`. **`CHALLENGE_WINDOW_SLOTS = 100`** (demo-sized; production posture is a D-log entry).
- **Events (invariant 8):** `BondFunded`, `RootPosted`, `ClaimPaid`, `RootChallenged`, `ChallengeResolved`, `BondSlashed` — every `*_after` field post-instruction truth (the A-042 contract).
- **Merkle recipe:** leaf = SHA-256(depositor ‖ amount_le_u64); node = SHA-256(left ‖ right), ORDER-SENSITIVE; verification walks the bits of `leaf_index`, so a proof asserts "this leaf is at THIS position". A sorted-pair tree was evaluated and REJECTED: a proof that only proved membership would let one leaf be claimed twice under two indices, each with its own valid proof and a fresh marker.
- **Errors, appended last (existing pins hold):** `RootExceedsVault` 6012 · `InvalidProof` 6013 · `WrongPayoutDestination` 6014 · `RootFrozen` 6015 · `ChallengeWindowClosed` 6016 · `RootNotProvenFalse` 6017 · `SlashExceedsBond` 6018.
- **Dependencies: none added.** A-058 is the open consequence: Session B must hand-write SHA-256 in `settlement.rs` (recommended — no `unsafe`, no dependency), declare the `sol_sha256` syscall (needs `unsafe` + an AUDIT entry), or get a dependency approved (workflow step 10).

**The suite — `programs/pool/tests/m11_doctrine.rs`, ten tests, each named as its sentence; the failure column is what the CI job log shows on this branch:**

| # | Test (line) | What it pins | RED on the stubs at |
|---|---|---|---|
| 1 | `root_whose_total_payout_exceeds_the_vaults_usdc_fails_closed_and_moves_nothing` (:545) | over by 1 µ$ ⇒ `RootExceedsVault`, no root born, vault/bond/operator wallet untouched; a root paying EXACTLY the vault posts | `fund_bond 10 USDC` must land |
| 2 | `claim_whose_inclusion_proof_does_not_verify_against_the_published_root_is_rejected` (:619) | a valid proof pays; another leaf's proof, a tampered amount, a proof from another tree, one flipped byte ⇒ `InvalidProof`, nothing paid | same |
| 3 | `one_leaf_funds_exactly_one_claim_a_replayed_claim_is_refused` (:762) | replay through BOTH doors refused at account creation; a leaf's OWN proof under another index ⇒ `InvalidProof`; each depositor paid exactly once | same |
| 4 | `payout_reaches_only_the_leafs_named_depositor_through_either_door` (:894) | direct door (backend absent) and assisted door (depositor absent) pay the NAMED depositor; a wallet the backend owns ⇒ 6014; the backend naming itself ⇒ 6013; a third key ⇒ `ExitAuthority` | same |
| 5 | `challenge_inside_the_window_freezes_that_roots_payouts_until_the_multisig_rules` (:1044) | `Challenged` ⇒ `RootFrozen` on both doors; neither the challenger nor the operator may resolve; "stands" unfreezes and pays; past the window (clock warp) ⇒ `ChallengeWindowClosed` and the root still pays | same |
| 6 | `bond_slashes_by_exactly_the_ruled_amount_for_a_root_proven_false_and_by_nothing_otherwise` (:1186) | a 2 USDC ruling moves exactly 2 USDC (record, vault, victim); second slash / never challenged / ruled to stand / awaiting ruling ⇒ 6017; an overstated ruling ⇒ 6018; a crank paying anyone but the victim ⇒ 6005 | same |
| 7 | `second_root_for_the_same_window_is_refused_roots_are_append_only` (:1412) | a different AND the identical root for the same window refused at account creation, the first root byte-identical in every field; the next window appends and the first root still pays | same |
| 8 | `every_state_change_is_reconstructible_from_emitted_events_alone` (:1533) | a subscriber fed ONLY decoded events equals the chain: vault, bond (record + vault), every field of two roots, every marker present and absent; an event census | `fund_bond 4 USDC` emitted NO event |
| 9 | `settlement_mints_no_transferable_receipt` (:1873) | after every settlement instruction: registry supply, the claim row, the USDC supply, the NonTransferable extension and the mint authority unchanged; the payout is USDC, exactly | `fund_bond 10 USDC` must land |
| 10 | `no_single_key_and_no_upgrade_authority_can_move_vault_funds_only_a_satisfied_program_instruction_can` (:1984) | the vault's authority is the pool PDA; raw transfers signed by the backend, the admin and the DEPLOYMENT (upgrade) authority key ⇒ Token `OwnerMismatch`; `sweep_vault` ⇒ Anchor 101; a valid claim moves exactly the leaf | same |

**Harness (`programs/pool/tests/common/mod.rs`, additive):** `boot()` extracted from `start()` (behavior unchanged) plus `start_with_context()` — only `ProgramTestContext` can `warp_to_slot`, and "inside the window" means something only next to its complement · settlement PDAs and state readers · six instruction builders and a raw Token-v3 transfer · the hand-encoded `Ruling` wire type · a test-side Merkle tree (the operator's off-chain view) · an event decoder (base64 + discriminators for all fourteen events) and an events-only `Subscriber` — invariant 8's instrument.

**CI verdict of the suite head `4ec3d42` — [run 45773107548](https://github.com/Phat1911/Tributary/actions/runs/45773107548): RED on the Rust job ONLY, and inside it on ONE binary.** `m11_doctrine`: 0 passed / 10 failed — nine at `invariant N — `fund_bond 10 USDC (a bonded operator backs its roots)` must land: transport transaction error: Error processing Instruction 0: Program failed to complete` (the `todo!()` abort, with no constraint answering first), and invariant 8 at `invariant 8 — `fund_bond 4 USDC` emitted NO event: every state change needs one`. Everything else in that job green: SBF build, fmt, clippy (both legs), pool 9 unit + 20 adversarial + 9 audit_session_c + 4 journey, the whole non-gateway workspace. **Every other job green:** Dashboard ✓, Dashboard E2E ✓, OpenAI SDK contract ✓, Supply chain ✓. One nuance, stated because it is easy to misread from the job list: the Rust job STOPS at its `Test` step (`--no-fail-fast` surfaces every failing binary, but the step itself fails), so its two later steps — the gateway binaries' tests and the m9c standing pins — do not run on this branch; neither touches or is touched by the pool crate, and both are green on main `3a57262`. CI's push trigger now includes the branch (M9-A precedent). **Close-out note (push + CI verdict, workflow step 8):** branch pushed; head `c627c56` (the diagnostic's removal + this entry) — **[run 45778330030](https://github.com/Phat1911/Tributary/actions/runs/45778330030), same shape: Rust red on `m11_doctrine` 0 passed / 10 failed with the ten messages above, every other test in that job green, the four other jobs green.** main is untouched (`3a57262`, nothing to push).

**Per-instruction evidence — the one thing `todo!()` stubs cannot show from the suite alone.** Every suite chain starts at `fund_bond` (a root needs a bond), so the suite never EXECUTES the other five account lists on this branch. A temporary diagnostic (`zz_m11a_diag.rs`: pushed, run, then REMOVED — commits `a3bafbc`, `5528ae9` and the removal commit; it is NOT part of the suite and seeds program state, which the suite never does) gave a bond record and one root to the CI-built program and drove all six instructions: each validates its accounts and aborts at its `todo!()` with no constraint answering first, and the constraint refusals land where the contract puts them (`WrongPayoutDestination` 6014; Anchor `ConstraintHasOne` 2001; `ConstraintSeeds` 2006 for both the root and the marker address) — [run 45777414921](https://github.com/Phat1911/Tributary/actions/runs/45777414921), `diag_every_settlement_instruction_validates_its_accounts_and_reaches_its_todo ... ok`. (Its first run, [45775989897](https://github.com/Phat1911/Tributary/actions/runs/45775989897), failed on the diagnostic's own seed: `#[account]`'s `AccountSerialize::try_serialize` already writes the discriminator. Lesson recorded here for whoever seeds a record again.)

**Local verification (no SBF toolchain on this machine — `target/rustup-home/toolchains/1.89.0-sbpf-solana-v1.54` is an empty 4 KB shell, so the RED evidence is the CI job log, per the kickoff):** `cargo check -p pool --locked` clean · `cargo fmt --all -- --check` clean · `cargo clippy --workspace --all-targets --locked -- -D warnings` clean (+ the geoip leg) · all five pool test binaries build and link · pool unit tests **9/9 on the host** — the 4 existing plus 5 contract pins that are GREEN BY DESIGN (they need no ELF): the six discriminators (proof the handlers are registered), the instruction arguments BOTH ways including the `Ruling` enum, the six event layouts against the decoder's offsets, seed distinctness, the bond record's size. Beyond that, an uncommitted host scratch (deleted at close) verified the test-side logic against the PROGRAM's own serialization: Merkle trees for n = 1..9 (every leaf's proof reaches the root; a leaf's own proof under another index does not; leaf order is authenticated), the decoder for all fourteen events fed from `Event::data()`, the subscriber's offsets, and all six harness-built instructions BYTE-IDENTICAL to discriminator + Anchor's generated argument structs.

**Three things the suite work caught before the founder meets them:** (1) a sorted-pair Merkle node admits a replay under another leaf index — the recipe binds every proof to its index (invariant 3 pins the attack); (2) `slash_bond`'s victim check as an ACCOUNT constraint would report an un-ruled root as a destination error (6005) instead of `RootNotProvenFalse` — it is a handler check after the status check, so a refusal names its reason; (3) the Anchor 1.2.2 multi-file constraints above.

**Known limitations / for the GO gate (nothing below was changed):**
1. **A-058 blocks Session B's Merkle verification** until the hash road is ruled (hand-written SHA-256 recommended).
2. **All ten tests go red at the chain's root (`fund_bond`)** — inherent to `todo!()` stubs; the per-instruction evidence is the temporary diagnostic above.
3. **Invariant 1's ceiling is the RAW vault balance** — necessary but not sufficient for rule 7: on-chain prepaid records are `deposited`, never net of consumption (A-059). The solvency half stays a books check.
4. **No paid-out accounting is pinned** (`Σ claims ≤ total_payout`): not derivable from the root alone, and `SettlementRoot` carries no `paid_out` field (the kickoff's field list). A tree whose leaves sum past `total_payout` can over-pay per leaf until the vault runs dry — Session B/C call.
5. **The exit-ceiling seam (`check_withdrawal_ceiling`, SPEC §11 "liquidity buffer") is not wired into `claim`** — no sentence names rate limiting; the seam stays a no-op.
6. **`challenge` is permissionless and free:** a false challenger freezes a root until the multisig rules (exposure bounded by the window and the multisig's latency); no challenger bond in the contract.
7. **`slash_bond` is permissionless to crank** (the ruling is the multisig's act; execution is anyone's) and a slash waits silently if the bond has less than the ruled amount (an earlier root's slash) — top-up via `fund_bond`, then the crank lands. A design choice for GO.
8. **No unbond instruction:** the operator cannot withdraw its bond (outside the ten sentences).
9. **D39's sweep ceiling (`sweep ≤ accrued equity.operator`) is not covered** — no sweep instruction in the contract.
10. **"Append-only" is pinned for same-window replays only:** windows need not be monotonic (not in the sentence).
11. **Invariant 9's observable limit:** a receipt minted to an address OUTSIDE the claim's account list cannot be observed without account enumeration (`BanksClient` has no `get_program_accounts`). The structural guard is the account list itself (no Token-2022 program, no mint account in `Claim`) plus the registry checks the test does make.
12. **Invariant 8 captures events by SIMULATION and then executes the same instruction** (the M8 c2 precedent — logs are only observable on the simulate path).
13. **Pre-existing, untouched:** `m9/doctrine-suite` is still in the CI push trigger although M9 closed; `m11_doctrine`'s failure lines are the GO-gate artifact, not a bug to fix.

**Site restoration (step 13):** nothing created outside the repo — no containers, volumes, networks, databases or bound ports (Docker inventory 9 containers / 22 volumes / 5 networks, identical before and after; `tributary-m9exp` appears in neither inventory — it was not running on this Docker when the session began, and nothing here touched it; the founder stack on 8111, the demo stack on 8112, `demo-upstream`, the named demo volumes and `tributary-m9exp` untouched). `/tmp/m11a-read` (folded PLAN excerpts, the CI job logs, the docker baseline) removed at close. In-repo: the untracked `zz_scratch_m11.rs` (host verification scratch) and `zz_m11a_diag.rs` (the diagnostic) are both gone — the latter only via its removal commit. Kept deliberately: `target/` build output (gitignored).

**Fast jump — M11-A touched:** `programs/pool/src/settlement.rs` (new — declarations + 5 unit tests) · `programs/pool/src/lib.rs` (six `todo!()` handlers, docs, glob re-export) · `programs/pool/src/errors.rs` (seven codes) · `programs/pool/src/state.rs` (one comment) · `programs/pool/tests/m11_doctrine.rs` (new — the suite) · `programs/pool/tests/common/mod.rs` (harness, additive) · `.github/workflows/ci.yml` (branch trigger) · `AUDIT.md` (A-058, A-059) · `PLAN.md` (this entry).

**Fast jump — what lives where (path + what it proves):**
- `programs/pool/src/lib.rs:631` `fund_bond` — the operator bonds its own USDC; no bond, no root.
- `programs/pool/src/lib.rs:645` `post_root` — invariants 1 and 7: the ceiling against the vault, and one root per window by address.
- `programs/pool/src/lib.rs:666` `claim` — invariants 2, 3, 4 and 5: the proof, one claim per leaf, both doors to the NAMED depositor, a frozen root pays nothing.
- `programs/pool/src/lib.rs:685` `challenge` — invariant 5: anyone freezes a root inside the window.
- `programs/pool/src/lib.rs:697` `resolve_challenge` — invariants 5 and 6: the multisig rules, and a ruling may not exceed the bond.
- `programs/pool/src/lib.rs:722` `slash_bond` — invariant 6: exactly the ruled amount, to the recorded victim, once.
- `programs/pool/src/settlement.rs` — every PDA seed, state layout, account list and event; the Merkle recipe and why proofs are bound to their index; four `todo!()` hash helpers; five host unit tests that pin the wiring and the wire shapes.
- `programs/pool/src/errors.rs` — the seven codes the suite pins (6012–6018).
- `programs/pool/tests/m11_doctrine.rs` — the ten sentences as tests.
- `programs/pool/tests/common/mod.rs` — the harness the suite (and Sessions B and C) share: boot/context, PDAs, builders, Merkle tree, event decoder, events-only subscriber.

### 2026-11-26 — CI-GREEN CHORE: dependency patch (RUSTSEC-2026-0592) + a deterministic A-052 pin

**Scope (kickoff):** main @ `ea45615` was red on two independent causes — the Supply-chain job (`cargo-deny`: `error[vulnerability]` RUSTSEC-2026-0592, `url` 2.5.8) and the Rust job (`m9d_fix_pins::a_refused_pick_falls_back_to_sibling_capacity`, an intermittent 429 at its fallback assertion) — both jobs red in runs [45636767185](https://github.com/Phat1911/Tributary/actions/runs/45636767185) and [45640919015](https://github.com/Phat1911/Tributary/actions/runs/45640919015). Two fix commits, one per fix (plus the close-out note that records the CI verdict). No product code, no `deny.toml` edit (the lettre and zstd ignores stay, warnings only, per the kickoff), no assertion touched (rule 6).

**Fix 1 — `url` 2.5.8 → 2.5.9, with its forced companion `form_urlencoded` 1.2.2 → 1.2.3 (dependency justification, workflow step 10; detail in AUDIT A-057):**
- **Why:** [RUSTSEC-2026-0592](https://rustsec.org/advisories/RUSTSEC-2026-0592) (published 2026-11-20) — `Url::origin()` recursed once per `blob:` prefix with no depth limit; one crafted URL aborts the process, and `catch_unwind` cannot catch a stack overflow. Patched ≥ 2.5.9. It had failed every CI run since 2026-11-25 08:14 UTC.
- **Kickoff deviation (reported at the tripwire; director ruled in-session, "land both"):** the kickoff prescribed `cargo update -p url` → "url 2.5.8 -> 2.5.9 and nothing else". That command is a NO-OP on current Cargo: conservative `-p` lets only the named package move, and `url` 2.5.9's own manifest requires `form_urlencoded ^1.2.3` while the lock pinned 1.2.2 — 2.5.9 cannot be locked alone (`cargo update -p url --verbose`: `Unchanged url v2.5.8 (available: v2.5.9)`).
- **What landed:** `cargo update -p url -p form_urlencoded` → `Locking 2 packages`. The lock diff is exactly two entries (version + checksum each; 4 lines out, 4 in), compared line by line against the pre-change lock; both checksums match the crates.io sparse index. No new crate, no manifest edit, no agave-lattice crate moved (A-033). `form_urlencoded`'s other dependents (`axum` 0.8.9, `serde_urlencoded` 0.7.1) already accept `^1.2.3`; `url` enters through `lettre` 0.11.23, `reqwest` 0.12.28, `sqlx-core` 0.9.0 and `solana-net-utils` 4.1.2.
- **Proof (red → green; local cargo-deny 0.20.2, advisory-db HEAD `b9b8a3c` of 2026-11-25):** pre-patch lock `cargo deny check advisories` → exit 1, `error[vulnerability]` … `advisories FAILED`; patched lock `cargo deny check` → exit 0, `advisories ok, bans ok, licenses ok, sources ok`. `cargo metadata --locked` exit 0.

**Fix 2 — `a_refused_pick_falls_back_to_sibling_capacity` made deterministic (setup only; the test's five assertions are byte-identical to the old file — a comment-free code diff shows exactly ONE moved block):**
- **Symptom:** intermittent `429 … bucket capacity exhausted: the estimated burn would exceed every depositor's contributed value` at the pin's fallback assertion (`m9d_fix_pins.rs:791` in the old file) — the Rust job in runs [45636767185](https://github.com/Phat1911/Tributary/actions/runs/45636767185) and [45640919015](https://github.com/Phat1911/Tributary/actions/runs/45640919015) (2 of the 7 Rust-job runs since the pin landed on 2026-11-19).
- **Diagnosis — the kickoff's theory CONFIRMED, with the arithmetic pinned. NOT a product fault:** routing weight is the GROSS `claims.contributed` (`ProRataByClaims::weight == contributed_micros`), so it never decays as a depositor burns: with A 40,000 and B 200,000 live, every hunt request picked A with probability exactly 1/6 and drained B the other 5/6 of the time. A can hold the 15,150 estimate until its 10th serve (9 serves leave 15,700; the 10th leaves 13,000), so the hunt ends at A's 10th pick and B's hunt serves m follow a negative binomial NB(r = 10, p = 1/6) (mean 50, sd 17.3). The five fallback requests all succeed iff 200,000 − 2,700·m − 2,700·4 ≥ 15,150, i.e. m ≤ 64 ⇒ P(fail) = P(m ≥ 65) = **19.0% per DB leg, 34.4% per run** in the i.i.d. model. Every such 429 was GENUINE exhaustion of BOTH depositors — the product's fallback was correct throughout.
- **Evidence 1 — instrumented replay of the pre-fix call sequence:** an untracked scratch harness was found in the worktree (`crates/gateway/tests/zz_scratch_a052.rs`, left by an earlier session, never committed) — it makes the old test's exact calls but records instead of asserting; run sequentially, 100 scenarios per DB leg against the throwaway PG, then removed at close on the director's ruling. Result: 34/200 pin failures (SQLite 13/100, Postgres 21/100). In all 200 scenarios: the hunt phase never failed, A exited at exactly 10 serves / 13,000 remaining, failure ⟺ B had served ≥ 65 times in the hunt, and the first failing request was exactly the first `j` with `B_rem − 2,700·(j−1) < 15,150`.
- **Evidence 2 — red control, the ORIGINAL test in the proof loop below:** 18/30 runs failed, every one on this single test at line 791 with the fallback 429 (13 SQLite legs, 5 Postgres legs), no other test ever. The rate sits above the i.i.d. 34% because the draw source is a process-wide counter: in a mostly-isolated binary the test consumes consecutive integers, so an outcome is a deterministic function of the counter's offset and neighbouring offsets share nearly all their draws (replaying the product's exact `mix64(draw) % 240,000 < 40,000` arithmetic per offset: 81/200 offsets fail, in stretches of up to 38 consecutive offsets). The observed rate therefore varies with machine and interleaving — CI 2/7, local 18/30, sequential replay 34/200 — while the mechanism is the same everywhere.
- **The fix (the kickoff's own suggestion):** B (200,000) is deposited only AFTER the hunt. A is drained alone — one candidate, so every hunt request is A's: exactly 10, no draw involved — then B enters with nothing spent, so the sibling capacity the pin needs exists by construction. The five fallback requests, their 200 assertion, both captured-count assertions and the two hunt assertions are untouched (rule 6); no product code, no other test.
- **Proof (kickoff: ≥ 30 runs of the test binary per database, zero failures):** `cargo test -p tributary-gateway --test m9d_fix_pins --locked` with `TRIBUTARY_TEST_DATABASE_URL` set — each run exercises all 8 tests on SQLite AND on Postgres. **50 runs: 50/50 green on SQLite, 50/50 green on Postgres** (400 test-leg executions per database, 0 failures; the Postgres legs are also visible from the DB side — 400 root pools written = 50 runs × 8 tests). Red control in the same loop: 18/30 failed.

**Verification (final tree = both fixes; rustc 1.99.1 through the repo's `target/cargo-home` toolchain; SQLite tempfiles + throwaway PG16 `tributary-pg-cigreen2` on :5434):** `cargo fmt --all -- --check` clean · `cargo clippy --workspace --all-targets --locked -- -D warnings` clean (only Cargo's two standing notes: the pool-profile warning and the pre-existing `proc-macro-error2` future-incompat report) · `cargo clippy -p tributary-gateway --features geoip --locked -- -D warnings` clean · `cargo deny check` all four checks ok · **full workspace dual-DB 198/198, 0 failed, Postgres half never skipped** — workspace minus gateway/pool 122 (common 19, crypto 9, control 72, ledger 22), gateway lib + bins 21, gateway integration binaries 55 (api 6, concurrency 1, m2 3, m4 12, m55 7, m5 6, m9_doctrine 9, m9d_fix_pins 8, streaming 3) · m9c standing pins exactly 2 green / 10 red (CI's contract) · `cargo check -p pool` clean. The pool SBF suite is deferred to CI (no platform-tools locally — M9-B / M9-D / v0.1.4 precedent).

**Known limitations / for the director (nothing below was changed):**
1. **The fallback branch is exercised by probability, not by construction.** Each of the five fallback requests still picks A first with probability 1/6 (weights are the gross contributions, 40,000 : 200,000), so in (5/6)^5 ≈ 40% of legs every request goes straight to B and the fallback branch is never entered; a no-fallback regression is caught by 1 − (5/6)^10 ≈ 84% of CI runs — unchanged since M9-D, and now visible because the flake no longer masks it. Making it deterministic needs an added assertion or a different fixture (e.g. B deposited at the five-request minimum, ≈ 26,000, would lift detection to ≈ 98% per run) — a test-doctrine question for the user (rule 6; the kickoff froze the assertions).
2. **A-056's tripwire date (2026-11-24) has passed:** lettre 0.11.23 still carries its six ignored advisories; 0.11.24 fixes all six. Untouched here per the kickoff ("leave their ignores") — the bump plus deletion of the six deny lines needs scheduling.
3. **Observation, NOT a finding (no PoC in a shipped suite; product code was out of scope):** four scenario loops running concurrently against ONE SQLite file produced transient `database is locked` (SQLITE_BUSY) errors — `POST /api/v1/orgs` answered 409 `slug … is already taken: … database is locked` (`crates/control/src/api.rs:260` maps ANY `insert_org` error to the slug-conflict variant, so a transient lock error masquerades as a taken slug) and `POST …/deposits` answered 500 (cause not logged in tests). The trigger was a scratch harness far more concurrent than any shipped suite; nothing shipped failed here (198/198 + 80 binary runs). Not investigated further — flagged for a director call, the more so if SQLite stays a supported production backend.
4. **Harness note, pre-existing (seen in CI's failing run and locally):** `ledger append-only role guard NOT applied (Postgres): tuple concurrently updated` prints when parallel tests arm the Postgres role split at the same instant; that binary then runs without the app-role split (a loud warning by design, A-031).

**Session residue (step 13) — removed at close, outcome confirmed in the close-out note:** my throwaway PG16 `tributary-pg-cigreen2` (+ its anonymous volume) and all `/tmp` scratch, together with the INHERITED residue on the director's in-session ruling: the untracked `crates/gateway/tests/zz_scratch_a052.rs` (already deleted before the gates — it failed `cargo fmt --all -- --check` with 7 hunks) and the running container `tributary-pg-cigreen` (127.0.0.1:5433, created 2026-11-25 21:57 by an earlier session, with its anonymous volume).

**Fast jump:** `Cargo.lock` (commit `30319c8`) · `crates/gateway/tests/m9d_fix_pins.rs` (commit `3a29811`) · `PLAN.md` · `AUDIT.md` (A-057 new, A-052 extended)

**Close-out note (push + CI verdict, workflow step 8; residue, step 13):** main pushed `ea45615..3a29811` — fix heads `30319c8` (deps) and `3a29811` (pin). **CI verdict of the fix head `3a29811`: GREEN — [run 45750240229](https://github.com/Phat1911/Tributary/actions/runs/45750240229), all five jobs:** Rust (fmt, clippy, test incl. the pool SBF suite and the gateway binaries; `standing pins hold: 10 exploit PoCs red (exploits dead), 2 clean probes green`; `m9d_fix_pins` 8/8 with `a_refused_pick_falls_back_to_sibling_capacity … ok`) ✓ · Dashboard ✓ · Dashboard E2E ✓ · OpenAI SDK contract ✓ · Supply chain (`advisories ok, bans ok, licenses ok, sources ok`) ✓. **Residue removal executed as planned:** `tributary-pg-cigreen2` and the inherited `tributary-pg-cigreen` removed together with their two anonymous volumes (Docker inventory 11 → 9 containers, 24 → 22 volumes, nothing else changed; the founder stack on 8111, the demo stack on 8112, `demo-upstream` and both named demo volumes untouched); the inherited scratch file and its 378 MB of orphaned build artifacts under `target/debug` deleted; all `/tmp` scratch deleted. **Kept deliberately:** `target/red-proof/ci-green-2026-11-26/` (40 KB of local evidence — proof-loop summaries, the 200-scenario replay, deny red → green, the CI verdict; gitignored, not committed).

### 2026-11-19 — M9-D: fix session (Session D, doctrine-suite step 14 — persona: senior payment-systems engineer)

**Scope (kickoff):** fix Session C's A-047..A-054 per the founder rulings — fallback in-doctrine (A-052), cargo-deny ignores-with-justification for humantime/chrono/lettre (no dep changes) — with the PoC file EDIT-FROZEN (red-first acceptance: a fix flips its PoC red). Verification target: full workspace dual-DB green, fmt/clippy clean, 10 exploit PoCs red + 2 clean probes green, main pushed, CI verdict recorded.

**Fixes landed (full detail + pin locations in AUDIT.md rows):**
- **A-047 (clamp + overage doctrine):** `adjust_for_owner` applies the A-020 settle delta under an atomic CASE clamp (`burned = min(contributed, burned + delta)`, both engines); /settle attributes per position `burned − settled_burn`, so absorbed overage never distributes — **proposed doctrine (report): the OPERATOR EATS the served-but-unbacked slice** (rule-7 posture: no payable is minted from capacity nobody backed; the usage row keeps honest metered cost; the key is exiled at exactly its contribution).
- **A-048 (idempotency):** migration 0017 `settle_runs` (UNIQUE pool+window; status pending/ok/failed; full response payload) + `settled_burn_micros` marking column (legacy rows seeded `settled = burned` — forward-only, history never re-settles). Identical-window retry replays the FIRST response verbatim (client-retry safe); any other window attributes only the delta; a CAS on the marking column makes concurrent overlapping settles lose loudly.
- **A-049 (books lead — director ruling + founder template approval 2026-11-19):** /settle posts `capacity.consume` (the NEW seventh template, founder-approved with the exact legs Dr claims.contributed / Cr revenue.usage — burned principal converts into the settlement pot; root pools have no renter prepayment, so prepaid.consume cannot fund it and the settle-template sufficiency guards would refuse) → `settle.fee` (equity.operator, D39) → `settle.distribute` per payout (payable.depositor); every posting references `m9.settle.<run_id>`. Positions become caches: new mirror invariants `claims == Σ(contributed − burned)` and `capacity == Σ contributed` hold by construction; solvency_report now sees payables (and honestly shows negative headroom until escrow is funded — the experimental slice's true state). Chart counterparties extended (SEED + 0017 UPDATE; mirror test green by construction; the six M7 templates byte-identical). Companion mechanical edit, covered by the approval: the posting.rs completeness unit-test array gained the new variant (strengthens the pin).
- **A-050 (resolution):** tie-break = longest family, then SMALLEST family, then SMALLEST provider (explicit legs — the comment's promise, no `>`-tuple game); admission is serves-based (`keys_for_model`): a key is a candidate iff its provider matches the resolved bucket AND some declared family serves the MODEL (hyphen-prefix counts); each key's ACCOUNTING bucket is its own longest serving declaration, so capacity/attribution/earnings book against the position its deposit opened.
- **A-051:** resolution SQL filters `status='active' AND health!='invalid'` — a key that may not serve cannot steer the bucket.
- **A-052 (fallback, founder-ruled in-doctrine):** refused pick excluded from candidates, request re-picks among survivors; 429 only when every candidate refused. The ruling authorized updating the suite's 429 pin — **no suite edit proved necessary**: invariant-6's single-depositor 429 is genuine exhaustion, verified green unchanged.
- **A-053:** `price_tokens` saturates at every step (no panic path, debug included); settle accumulates in i128 with checked i64 narrowing (overflow = loud 500); the clamp bounds hostile attributions at contributions (settle 200 OK on the PoC's two 6.15e16-token rows); zero-report floor — usage 0/0 bills the reserve estimate (doctrine: un-metered ≠ free; residual: honest-tiny positive reports still drain slowly, and member token quotas still aggregate reported tokens — both revisit with D23).
- **A-054:** addressed by the permanent successor pins (`crates/gateway/tests/m9d_fix_pins.rs`, 8 additive tests) closing gaps (a),(b),(d),(e),(f); gaps (c),(g) remain routed doctrine questions (Session B suites untouched).
- **cargo-deny (founder ruling):** A-055 (humantime/chrono unmaintained — dev-only via the agave lattice) + A-056 (lettre 0.11.23's six advisories — release-path alert-email sender; operator-configured-relay exposure honestly stated) ignored in `deny.toml` with inline rationale; **revisit tripwire: 2026-11-24 (after the Dev3pack hackathon, before M11-A / Nov 26 travel) — bump lettre ≥0.11.24 and DELETE the deny lines; CI red on deletion = tripwire fired.**
- **CI (infra only, no test edits):** the frozen m9c binary left out of the green-must-pass run (workspace-minus-gateway + enumerated gateway binaries), and a new "Standing pins" step enforces exactly 10 red exploit pins + 2 green clean probes — the exploit-death state itself is now CI-verified.

**Verification:** full workspace dual-DB GREEN on the fix branch (gateway: unit 18+3, api 6, concurrency 1, m2 3, m4 12, m55 7, m5 6, m9_doctrine 9/9, m9d_fix_pins 8/8, streaming; ledger: 22; control: 60; common 19; crypto 9 — SQLite tempfiles + throwaway PG16 `tributary-pg-m9d` on :5433); `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo deny check advisories` clean with the ruled ignores; m9c standing pins 10 red / 2 green (each red verified to fail at its intended exploit assertion — details in AUDIT rows). Pool SBF suite deferred to CI per the M9-B precedent (no platform-tools locally). Doctrine mid-flight catch: `MIN(a,b)` is SQLite-scalar but PG-aggregate-only — the clamp is a portable CASE (the dual-DB run caught it; single-engine testing would have shipped a silent settle failure on PG).

**Known limitations / doctrine proposals for the director:** (1) a failed settle posting phase leaves books-ahead drift with the run tombstoned 'failed' (append-only books; the run row + audit log name the recovery; guard failure requires books↔positions drift, i.e. manual SQL damage); (2) legacy pre-fix `earned_micros` (positions-only era) was never posted — operator cleanup item, deliberately not re-settleable; (3) the m9_doctrine header's "bucket-positions — the off-chain books (D43)" wording is stale under books-lead — doctrine question for the user (rule 6; no test edited); (4) A-053 residuals (honest-tiny drains; member quota aggregation) await D23; (5) solvency now goes red on root pools with settled payables until escrow exists — honest, but the demo stack's nightly check should be watched.

**Fast jump:** `migrations/0017_m9d_settle_runs_and_clamp.sql` · `crates/ledger/src/posting.rs` (capacity.consume) · `crates/ledger/src/chart.rs` · `crates/ledger/src/settle.rs` · `crates/gateway/src/depositor_mode.rs` · `crates/gateway/src/chat.rs` · `crates/gateway/src/m9_api.rs` · `crates/gateway/tests/m9d_fix_pins.rs` (new) · `.github/workflows/ci.yml` · `deny.toml` · `AUDIT.md` (A-047..A-054 flipped, A-055/A-056 new) · `PLAN.md` (this entry).

### 2026-11-18 — M9-C: adversarial audit (Session C, doctrine-suite step 14 — persona: systems-accounting attacker)

**Scope:** attack the depositor mode as shipped in v0.1.4 (M9-B merged at cf3f991), through public surfaces, per the 11-invariant doctrine checklist (taxonomy, not scope), the implementer's four bait items, the composition surfaces, THE SETTLE ASYMMETRY probe, and the suite-recursion pass. Rules of engagement honored: no code or existing test edited; every finding ships as a reproducible PoC with severity; attempted-clean categories documented.

**Findings (full four-pointer detail in AUDIT.md A-047..A-054):**
- **A-047 (HIGH, inv 6):** per-bucket overdraw via the estimate-vs-actual gap — the ceiling holds only at reserve, the settle half has no clamp, and `estimate_tokens` under-counts (array-content prompts ≈ 0 prompt tokens; 256-token default completion). One ordinary member request overdraws 9× (2,700 burned vs 300 contributed).
- **A-048 (HIGH, inv 2/7):** no settled-marking on windows — the same rows settle twice, `earned_micros` double-credits; a settle-retry suffices.
- **A-049 (HIGH, inv 1/7 + rule 7):** /settle updates positions and posts NOTHING — after real traffic `payable.depositor` doesn't exist while earned > 0; the "ledger side pinned by the ledger suite" is a tautology (the suite posts through the trait itself); rule-7 solvency recomputes from entries only → structurally blind to settled payables.
- **A-050 (HIGH, inv 8/5):** bucket-resolution ordering is attacker-playable with free strings — provider "zzz" at 1% weight captures 100% of a model's traffic (longest-family leg exiles honest prefix-declarers; comment/code drift at the tie-break selects the LARGEST provider/family while the comment promises smallest).
- **A-051 (MED, inv 5):** revoked keys' zombie declarations keep winning resolution (the model→bucket scan has no status filter) — a revoked 1% key bricks a model while a funded healthy key never dials.
- **A-052 (MED, inv 5/8):** picked-then-refused 429 without fallback — members refused while a sibling depositor holds ≥ the estimate unspent; related: deposits smaller than one estimate can never serve.
- **A-053 (MED, inv 1/6):** upstream-reported usage trusted — two hostile rows overflow i64 in /settle (panic: every window containing them un-settleable forever); zero-usage upstream serves free forever AND is a member token-quota bypass primitive.
- **A-054 (LOW):** suite recursion — the 14 green tests miss exactly the above: single-settle journeys, self-posting ledger reconciliation, no family-adversary journeys, no adversarial body shapes, and the invariant-6 journey PINS the no-fallback 429 as doctrine.

**Bait verdicts:** #1 overlapping windows double-credit → VERIFIED (A-048). #2 picked-then-refused 429 no fallback → VERIFIED (A-052). #3 capacity stubs single-bucket-key only → REFUTED for the product path (the chat path always uses the bucket-aware `_for` forms; the single-argument forms have zero callers and error loudly on multi-bucket keys — dead contract surface, noted). #4 pricing v0 flat/token games/zero-cost → VERIFIED two ways (A-053; flatness itself is the director-owned D23 placeholder, no finding).

**Attempted-clean (documented silence):** fee-band edges 499/500/1000/1001 + window validation (clean, pinned); mid-window deposits/weight mutations (clean — new depositor earns 0, earnings follow burns); rows↔positions reconciliation per (depositor,bucket) — exact to the micro-unit even in the overdraw PoCs; concurrent conditional-UPDATE races at reserve (A-020 shape holds on both engines — the break is the estimate gap, not a race); reserve→pause→settle→resume (settle works on disabled pools — noted, no invariant break: the burns already happened; deposits correctly refuse); settle-window vs usage-row ts boundary (rows carry response-end ts; in-flight rows attribute to the next window — consistent once A-048's marking exists); degenerate family strings (empty rejected at deposit; unparseable skipped); cross-pool depositor_id links (pool-scoped positions, additive, no break); zero-burn depositors can never receive a remainder micro (derivation verified); modulo bias in the draw (negligible, within suite tolerance); dead single-arg stubs (above); http:// base_url — PRE-EXISTING V1 surface (control path identical), key-over-plaintext possible, not an M9 regression (report note).

**Verification:** PoC suite `crates/gateway/tests/m9c_audit_poc.rs` 12/12 dual-DB green (SQLite tempfiles + throwaway PG16 `tributary-pg-m9c` on :5433) — assert-the-exploit convention (green = vulnerability present; each flips red when its finding is fixed; M8 Session-C standing-pin precedent). Full gateway suite dual-DB green after (additive-only). `cargo fmt --check` clean; `cargo clippy -p tributary-gateway --all-targets -- -D warnings` clean. Rig: stashed Linux toolchain at `target/cargo-home` per PLAN's export line; no Windows-side cargo use; throwaway PG removed at close (step 13).

**Fast jump:** `crates/gateway/tests/m9c_audit_poc.rs` (new — all 12 PoCs) · `AUDIT.md` (A-047..A-054) · `PLAN.md` (this entry). No source file touched.

**Post-audit ruling (2026-11-19, director, on A-049):** fix direction RULED — settle posts the books automatically (settle.distribute → payable.depositor; settle.fee → equity.operator per D39); positions become true caches (books lead). Operator-approval staging is an M10+ option, built stage→approve→post, never cache-leads-books. Fix session pending; MUST land with A-048's window marking/idempotency (coupling recorded in AUDIT A-049 — auto-posting over unmarked windows would double-post the ledger).

### 2026-11-11 — v0.1.4 TRUTH RELEASE: docs-honesty + small hardening (GO with 4 approved defaults + reviewer-readiness addendum)

**Scope.** Make the external review's docs-honesty findings obsolete, as a tagged release shipping before the Nov-12 video record. GO granted 2026-11-11; all four worker defaults approved (devexchange docs frozen verbatim; devnet-soak stays 0.1.3; `TRIBUTARY_ADMIN_PASSWORD`; scrub unchanged with guards-first documented). Addendum executed: repo topics, public devnet receipts, "For reviewers" release block, generic external-review credit.

**Items (commit each):** chore 0 `eb617d0` AGENTS.md Session-language sweep + `50d50c5` DevExchange artifacts (deployed-Pages sources) committed frozen with a Current-state note · README restructure `658cd65` (Why-leads: free self-hosted V1, V2 gated roadmap not storefront; "Business model, plainly" — exact director text; classifier qualifier VERIFIED TRUE first: m4_journeys "Dormant by default" pin + `/api/v1/me/disclosure`; depositor paragraph + devnet receipt — program id checked against `programs/pool/src/lib.rs` `declare_id!`) · storefront fix (no commit — repo side): description PATCHed, old value **"Self-hostable AI token pools — govern your org's AI usage with per-member keys, quotas, and leak detection; or deposit surplus credits into a Solana-settled marketplace."**, old topics (20): ai-gateway, api-key-management, axum, leak-prevention, llm, llm-gateway, llm-proxy, llmops, nextjs, open-core, openai, openai-compatible, postgresql, rate-limiting, rust, self-hosted, solana, spend-management, sqlite, usage-tracking → new (11): rust, solana, anchor, ai-gateway, llm, api-key-management, proxy, openai-compatible, self-hosted, token-pool, governance · create-admin env password `c74c44d`+`d5beb9e` (argv wins > env, empty env = unset, error names both paths; 3 unit tests) · deploy truth pass `2cc1b9f` (LAN secure-cookie + VPS no-cleartext warnings; tailscale-serve REQUIRES `TRIBUTARY_TRUSTED_PROXIES` — XFF behavior verified against tailscale's `ipn/ipnlocal/serve.go` `addProxyForwardedHeaders` (sets XFF to the caller's real tailnet addr, discards spoofed values); compose truth: plain `up -d` prefers prebuilt, `--build` is the deliberate source path — README/compose-header/deploy-guide now agree; leak-detection false-positive note — `DistinctIpConcurrency` verified **alert-only** in `control/src/security.rs`; healthcheck section de-staled) · audit_log append-only `5dfa8ae` (see A-046; `run_append_only_guards` extends the M7 pattern — SQLite triggers + PG `tributary_app` grant split on `audit_log`, NO numbered migration; burglar test `direct_update_and_delete_on_audit_log_must_fail` in `crates/ledger/tests/append_only.rs`) · threat model `5a55b82` (`docs/threat-model.md`: audited-how / NOT-yet-reviewed (key-vault crypto, gateway auth path — seeking external review) / bus factor / GitHub-issues-only; linked from README + SECURITY).

**Release-build finding (fixed same session, pre-existing class).** The FIRST tag after M8 broke the image build: release run 42976342936 on `4d37e39` failed 0.2 s into `cargo build --locked` — `programs/pool` joined the workspace (M8) after v0.1.3 and the Docker context never copied it. Fix `cc9f479`: `COPY programs/pool/Cargo.toml` + `programs/pool/src` (the manifest walk needs every member's manifest AND `src/lib.rs` for lib auto-discovery — manifest-only reproduced the failure in a `rust:1-slim` container; with src, `cargo metadata --locked` exit 0). **Tag re-pointed once** (`v0.1.4`: 4d37e39 → cc9f479) BEFORE any artifact existed — the failed run published nothing and no release object existed. Second release run 42977385631: **success** — GHCR `0.1.4` + `latest`, multi-arch.

**Verification.** Toolchain: no native cargo on this box and `$HOME` is sandbox-read-only → rustup stable 1.99.1 installed INTO `target/cargo-home` (gitignored scratch, registry cache reused; reuse via `RUSTUP_HOME=$PWD/target/cargo-home/rustup CARGO_HOME=$PWD/target/cargo-home PATH=$PWD/target/cargo-home/bin:$PATH`). `cargo fmt --check` clean; `clippy -D warnings` clean; **full workspace dual-DB GREEN** (throwaway PG16 `tributary-pg-v014` on :5433, since removed): burglar tests green on BOTH engines — PG half explicitly verified (`tributary_app` role present, no skip line); create-admin unit tests 3/3; full workspace exit 0 (gateway 68, ledger 22 incl. both append-only alarms, control suites green). Pool suite deferred to CI per the M9-B precedent (no SBF platform-tools locally; the stale `target/deploy/pool.so` was never loaded — pool shares zero tributary code, verified) — **CI Rust job incl. the pool suite GREEN on the tagged head**.

**CI verdict of the tagged head (`cc9f479`): run 42978859426 — GREEN.** Rust (fmt/clippy/test incl. pool) ✓, Dashboard ✓, SDK contract J7 ✓, cargo-deny ✓; the E2E leg failed ONCE (`f1-budget-wire` "minute rule exists" — minute-window UI flake; the only diff vs the fully-green 4d37e39 run 42975735483 is a Dockerfile COPY line, which cannot reach Playwright) and went green on the standard failed-job re-run — all five jobs green. Interim: run 42975735483 on `4d37e39` fully green; Landing Pages redeployed (eyebrow v0.1.4 verified live).

**Smoke on the RELEASED image** (throwaway 8113 + throwaway volume, torn down after): `docker pull` of published `0.1.4`; healthz 200 after boot-migrations; **create-admin via `TRIBUTARY_ADMIN_PASSWORD` then login 200 with that password on a fresh volume** — the shipped hardening proven end-to-end on the released binary; dashboard 200.

**Demo re-pin (8112).** `tributary-demo` recreated on `0.1.4` IN PLACE: same volume (`tributary-demo-data`), same master key (captured via docker inspect, never printed), same `TRIBUTARY_TRUSTED_PROXIES=172.16.0.0/12`, same `8112:8080`, same restart policy, same dual-homing (bridge@172.17.0.2 + demo-net@172.28.0.3); **healthz 200**. Founder's 8111 untouched (`latest`, pulls at his convenience). `demo-upstream` untouched.

**Release object:** https://github.com/Phat1911/Tributary/releases/tag/v0.1.4 — first release WITH notes (v0.1.3 shipped tag-only). Notes carry the two required sentences (depositor mode experimental; adversarial audit runs this week), the docs-truth headline, generic external-review credit (no names/tools), the devnet receipt line, the For-reviewers block, and the known line (devnet-soak's own 0.1.3 = standalone ops client, not drift).

**Known/residue (step 13).** ~~E2E minute-window flake: one occurrence, re-run green — same family the v0.1.3 flake guard covers; watch, do not touch without a second occurrence.~~ **RESOLVED same day (director rule-6 order, `67a4f4d` + `2f551b1`): the second occurrence (run 42983184933, docs-only push) voided watch-only. Diagnosis from job logs: NOT the burst leg — the F1 minute-rule read raced the dialog's own save POST (~9 s after stack boot on a cold runner; the still-open dialog's "Minute (requests)" combobox label satisfied the old text check; the immediate API read found no rule). Fix is setup-only: `expect.poll` the rule into existence before the pins; every assertion unchanged ([200×5, 429×3] stays); day leg untouched (its "100,000 tokens" check can only match the rendered table, which implies the POST landed). Proof: 5× consecutive green locally, fresh stack per run (`TRIBUTARY_BIN` + `scripts/e2e_j6.py`), every burst leg exactly `[200,200,200,200,200,429,429,429]`.** **Separate infra outage, same evening (unrelated to the fix, tests never ran in it): GitHub-hosted runners lost Ubuntu-mirror DNS for 75+ min (14:30–15:56 UTC; azure AND primary mirrors unresolvable on 5+ fresh runners; npm/crates.io fine) — apt exit 100 in `playwright install --with-deps`, 5 E2E reds. A mirror-swap attempt (`1bb15c0`) did not help (primary mirror also down); final fix `2f551b1`: the deps install is best-effort with a plain-browser-download fallback — legitimate because the ubuntu-24.04 image ships SYSTEM Chromium 156 (the full lib set), and Playwright validates host libs at launch so a real gap fails loud. Proven live: the warning fired mid-outage, fallback installed, tests ran for real (`F1_BURST=[200,200,200,200,200,429,429,429]`, 2 passed). CI verdict of the fix head `2f551b1` (run 43051635141): ALL FIVE JOBS GREEN incl. E2E.** Session residue removed: smoke container+volume, throwaway PG. Kept deliberately: `target/cargo-home` toolchain (gitignored cache for future sessions — Windows-cache rules unaffected, nothing lives under C:). Fast jump: `AGENTS.md` · `demo/devexchange-slides.html` · `docs/devexchange-{demo,runbook}.md` · `README.md` (×2) · `crates/gateway/src/main.rs` · `deploy/README.md` · `docker-compose.yml` · `docs/pilot-support.md` · `crates/common/src/db.rs` · `crates/ledger/tests/append_only.rs` · `AUDIT.md` (A-046) · `docs/threat-model.md` (new) · `SECURITY.md` · bump set `Cargo.toml`/`Cargo.lock`/`apps/dashboard/package.json`/`landing/index.html` · `deploy/Dockerfile` · repo description+topics (GitHub side) · tag `v0.1.4` + release + GHCR `0.1.4`/`latest` · demo 8112 re-pin.

### 2026-11-11 — M9-B: implement to green (Session B, doctrine-suite step 14) — 13/13 red → 0, merged to main

**Session B, fresh context, GO granted with the director's two scope notes accepted** (pricing v0 documented placeholder, D23 owns the real table; /settle's positions-direct update without ledger posting is accepted M9 asymmetry, flagged to Session C). First move per kickoff: the director's dirty AGENTS.md amendment ("Director verification depth" clause) turned out destroyed by a Nov 5 reset — re-supplied verbatim by the user at GO and committed standalone (`d5d17f5`). Red baseline re-confirmed on the branch before any code (5 ledger + 8 gateway failures, invariant 9 green by design).

**Implementation, additive only (invariant 10), zero test edits, stub signatures byte-identical:**
- **D43 split** (`crates/ledger/src/settle.rs`): `split_settlement` aggregates burns per bucket; burnless buckets' distributable is RETURNED (undistributed, never crossed); fee = per-bucket `dist × bps / 10_000` (i128, clamped); after-fee remainder splits pro-rata by burns with **largest-fraction remainder allocation, depositor-id tie-break** — deterministic to the micro-unit (same inputs ⇒ byte-identical split, M11-reproducible); `bucket_positions` = ordered read of migration 0016's positions.
- **Routing** (`crates/gateway/src/depositor_mode.rs`): `ProRataByClaims::weight == contributed_micros` (claims.contributed — the ONE source); `select` mixes ANY u64 draw via **splitmix64** (validated pre-implementation against the suite's exact tolerance math: 307/400, 65/80, 4/2) then walks the cumulative ladder; hot path draws from a process AtomicU64 counter — no RNG, no new deps. Bucket registry: family serves model iff equal or **hyphen-prefix**, longest match wins (deterministic tie-breaks); key registry LEFT-JOINs positions so missing books are visible, not silent; `eligible_keys` = bucket ∧ healthy ∧ remaining > 0 (revoked/invalid never dialed).
- **Overdraw guard** (same file): ONE conditional UPDATE (`burned + estimate ≤ contributed`) per A-020's atomicity shape; refusal fails CLOSED (unknown key/missing position/DB error ⇒ never dial). Stub single-argument reserve/settle/release implemented for single-bucket keys + bucket-aware `_for` forms for the hot path (multi-family keys).
- **Pricing slice v0** (`PRICE_MICROS_PER_TOKEN = 150` µ$/token = $0.15/1K): deterministic integer cost, **root pools only** — V1 keeps cost 0 (pinned by gateway tests/api.rs "cost stays 0 until pricing (D23)"; placeholder owned by D23).
- **Metering + attribution** (`crates/gateway/src/db.rs`, `chat.rs`): `insert_usage_event_attributed` writes the three migration-0016 columns at the A-020 response-end settle moment (NEVER at reserve — invariant 11); `UsageEvent` struct untouched (existing tests construct it); reserve→upstream→settle/release lifecycle wired symmetrically on buffered AND streaming paths; quota_buckets untouched (the tripwire stays armed).
- **Endpoints** (`crates/gateway/src/m9_api.rs`, merged into `build_full_app` — gateway owns them because the deposit path posts `deposit.capacity` via `SqlLedger` and settle calls the ledger split; control stays ledger-free, no cycle, no second SQL truth): `POST /api/v1/pools/{id}/deposits` (root-gated; envelope-seals the key; mints-or-links depositor; per-bucket positions splitting the contribution deterministically, upsert-additive; posts deposit.capacity through the books; 201 `{depositor_id, provider_key_id, key_prefix}`), `GET …/bucket-positions` (root-gated read of the books), `POST …/settle` (root-gated; batches the window's attributed rows; per-bucket distributable = Σ row cost; rule-8 band 500–1000 bps enforced at the surface; credits payouts onto positions; the contract JSON). `POST /api/v1/pools` gains optional `kind: "org"|"root"` (default org — V1 byte-identical).
- **V1 preservation (invariant 9):** family pools keep first-available selection, unattributed rows, cost 0; both M9 endpoints refuse org pools (4xx); the V1 pin rides every suite run.

**Verification:** both doctrine suites 13/13 red → 0 dual-DB (ledger 5/5, gateway 9/9 including the green-by-design V1 pin); **full workspace (excluding pool) 186/186 dual-DB twice consecutively** (SQLite tempfiles + throwaway PG 16 container `tributary-pg-m9b` on :5433; one transient m9_doctrine flake in the first under-load pass, clean in both confirmation passes); `cargo fmt --check` clean; `clippy --workspace --all-targets -D warnings` clean (excluding pool); `clippy -p tributary-gateway --features geoip -D warnings` clean; `cargo check -p pool` clean. **Pool test suite deferred to CI on the merged head** — its dev-dep chain (solana-program-test → openssl-sys 0.9.117 from-source) cannot build under the Windows-cargo interop (Git-for-Windows perl is stripped: Locale::Maketext::Simple injectable via PERL5LIB, ExtUtils::MakeMaker not worth mutating the machine for; pool shares zero code with M9's crates). Local command that would clear it: a Linux-side toolchain (WSL rustup) or Strawberry Perl. **Dependencies: none added** (splitmix64 is 5 in-file lines; ledger path dep predates this session).

**Known limitations / honest notes:** (1) pricing v0 flat rate, root-only, D23 replaces; (2) /settle does not post to the books and does not mark windows settled — overlapping windows double-credit `earned_micros` (operator discipline until Session C rules); (3) single-argument reserve/settle/release stubs are single-bucket-key only (the `_for` forms serve the hot path); (4) a picked-then-refused key 429s without fallback to other eligible keys (pick-then-reserve is the contract order — Session C bait); (5) throwaway PG container + PERL5LIB folder removed at close (step 13 below).

**CI close-out (pre-existing main rot, fixed to deliver DONE-WHEN's green):** the merge run ([42939023038](https://github.com/Phat1911/Tributary/actions/runs/42939023038)) came back **Rust (fmt/clippy/test incl. the full pool suite on ubuntu) ✓, Dashboard ✓, SDK contract ✓** — with exactly the two failures main already carried BEFORE the merge (run 42730913935 on 90eb4d8): (a) **cargo-deny / RUSTSEC-2026-0459** — libsqlite3-sys bundled-source CVE; NOT-applicable-by-configuration here (no `bundled` feature anywhere; sqlx-sqlite 0.9.0's own dep activates only pkg-config/vcpkg; every build links SYSTEM sqlite; the fixed 0.39 is unreachable inside sqlx's `<0.38` range and 0.9.0 is the latest release) → `deny.toml` ignore with the full rationale inline + **A-045** in AUDIT.md (workflow step 10 justification: zero new dependencies — the change is one documented ignore line whose deletion at the next sqlx bump is the revisit test); (b) **E2E / Playwright cannot target ubuntu26.04** (ubuntu-latest image rollover) → the e2e job pinned to `ubuntu-24.04` with an unpin comment. Both fixes committed to main AFTER the merge — the merge commit itself stays pure doctrine.

**Fast jump:** `crates/ledger/src/settle.rs` · `crates/gateway/src/depositor_mode.rs` · `crates/gateway/src/m9_api.rs` (new) · `crates/gateway/src/{chat,db,routing,lib}.rs` · `crates/control/src/api.rs` (pool kind) · `AGENTS.md` (chore d5d17f5) · `deny.toml` + `.github/workflows/ci.yml` + `AUDIT.md` A-045 (CI close-out) · `PLAN.md` (this entry).

### 2026-11-02 — M9-A: doctrine suite (Session A, step 14) — EXPECTED-RED on `m9/doctrine-suite`, GO gate pending

**Doctrine-suite mode, first execution.** Session A restated doctrine + plan, waited for GO (granted with two refinements: ONE contributed-value name — `claims.contributed`, `capacity.contributed` never read by routing/overdraw; report shape = table of contents). Then, same-commit: **D43** filed (SPEC Appendix A) + **A-044** (step-9 ambiguity: "pro-rata accounting" undefined — resolved by D43; A-030 precedent); **migration 0016** (additive attribution columns: `provider_keys.depositor_id/model_families`, `usage_events.depositor_id/bucket_provider/bucket_model_family`, new `depositor_bucket_positions` — schema-as-interface, logic stays B's); **stub interfaces** (`crates/ledger/src/settle.rs` — Bucket/BurnRecord/Payout/SettlementSplit/`split_settlement`/`bucket_positions`; `crates/gateway/src/depositor_mode.rs` — bucket registry, per-depositor key registry, `RoutingPolicy`/`ProRataByClaims`, `attribute_period_burns`, bucket-capacity reserve/settle/release; gateway now depends on tributary-ledger for the single `Bucket` type — no new external deps); **the suite** — 14 tests across `crates/ledger/tests/m9_doctrine.rs` (invariants 1,2,3,4,7 — seeded-LCG property walks + posting-through-the-trait reconciliation) and `crates/gateway/tests/m9_doctrine.rs` (invariants 1,2,3,5,6,8,9,10,11 — public-API journeys per step 12, M9 endpoint contract documented in the file header). Invariant 9's V1-preservation pin is GREEN on the branch BY DESIGN. CI push-trigger extended to the branch (expected-red run = the GO-gate artifact).

**Session B contract:** implement the stubs + the M9 public endpoints (deposits/settle/bucket-positions/kind:"root"); NEVER edit a test — a test that must change is a doctrine question and routes through the user (rule 6 extended).

**Fast jump:** `migrations/0016_m9_depositor_attribution.sql` · `crates/ledger/src/settle.rs` + `lib.rs` · `crates/gateway/src/depositor_mode.rs` + `lib.rs` + `Cargo.toml` · `crates/ledger/tests/m9_doctrine.rs` · `crates/gateway/tests/m9_doctrine.rs` · `SPEC.md` (D43) · `AUDIT.md` (A-044) · `PLAN.md` (this entry) · `.github/workflows/ci.yml` (branch trigger).

### 2026-11-02 — M8 CHORE: devnet program synced to HEAD 460fd00 (Session C fix live on-chain)

**Ops, not code — nothing merged from this chore** (the docs commit below is
the only repo change). Preflight → buffer → **extend_program +65,536 B** →
**2-of-2 upgrade** (founder + teammate-rotated, vault-signed, [Solscan](https://solscan.io/tx/576QrbHPxvDB3P63nHedFA5hDqVBXJchzkauEvrpAJzHUyyDCnfd6KhXaRw2gePS7RF2NQh2eM5qsgj6wwCdeaP5?cluster=devnet))
→ resume via 2-of-2 → live verification, all evidence in
`docs/devnet-soak-m8.md` (sync section). Headlines: pool state INTACT
(upgrade-safe diff: state.rs zero-diff, errors append-only); **A-042 proven
live** (emitted `supply_after`/`vault_balance_after` == on-chain truth on
`record_contribution`/`prepay_renter` — the old code emitted pre-instruction
values); burn + prepay/refund round-trip exact through both doors; pool ends
ACTIVE; authority stays the Squads vault throughout.

**New ops knowledge (dated):** (1) first-ever GROWN ELF on this deployment —
the loader's ProgramData needed `extend_program` BEFORE the upgrade, and the
runtime allowlists loader instructions from CPI to Upgrade/SetAuthority/Close
ONLY, so extend is a top-level payer tx (no authority account at all);
allocation now 348,781 B (M11 headroom). (2) Buffer authority parses at bytes
5..37 (option tag at 4) — the soak client's 4..36 is off by one (A-040
class; noted in the runbook, client untouched per "nothing merges"). (3) Two
stale approved-but-unexecuted ms proposals (indexes 7, 8) from refused
executes are harmless on-chain residue; clearable by a
`transaction_accounts_close` vote. (4) `initialize` on the born singleton
refuses for ANY signer at anchor's init constraint — the A-041 gate guards
unborn pools (rig-pinned `c1`) and every future deployment.

**Verification:** upgrade tx slot 517758888 → 517759050 (authority vault);
pool admin/backend/layout byte-identical; event-mirror == chain truth on both
probed instructions; vault/record back to 0 after the round-trip; deploy key
recovered the buffer rent via the upgrade spill (7.82 SOL). Local tree
clean; the throwaway ops driver was deleted (never committed).

**Fast jump:** `docs/devnet-soak-m8.md` (sync section) · `PLAN.md` (this entry).

### 2026-11-02 — M8 SESSION C FIX: A-041 + A-042 fixed (GO-gated), A-043 accepted-until-M10

**Persona & gates.** Fix session per kickoff (senior-Solana-engineer persona; each Session-C PoC treated as a spec). GO proposal accepted at the gate for: A-041 direction **(a)** — implemented as the *deployment authority = the program's upgrade authority* (the naive "admin-arg-signs" variant was ruled out with evidence: an attacker self-appoints); and A-042 fixing the fourth same-class emitter (`ClaimsBurned.supply_after`) in the same pass. Rule 6 honored: c1/c2a-c2c flipped from finding PoCs to regression pins exactly per the handoff contract (same attacker chair, assertions on the healed behavior — each flipped assert is STRONGER than the one it replaced); c3-c7 are byte-identical to Session C's authorship.

**A-041 (med → fixed).** `initialize` now requires the deployment authority's signature: the handler derives the loader ProgramData PDA of `crate::ID`, owner-checks it against the BPF Upgradeable Loader, parses the 45-byte header (u32 variant 3, u64 slot, u8 option at 12, authority at 13..45 — the A-040 lesson's pinned layout), and requires that authority to be the tx signer (`PoolError::DeploymentAuthority` = 6011, appended last so every pinned code holds). On devnet the authority is the Squads vault, so the pool's BIRTH is a 2-of-2 voted action — the deploy→initialize window is not capturable, refusals are atomic (singleton proven unborn), and an immutable deployment refuses outright. Rig evidence for the mechanism: program-test's genesis helper pins the authority to the unsignable `Pubkey::default()`, so `common::start()` now overrides the ProgramData via the sanctioned `add_account` path with a per-process random keypair (45-byte header + ELF, hand-rolled bytes). Flipped pin `c1_initialize_front_run_is_refused_without_deployment_authority`: attacker self-appointment AND substituted-program_data both refuse with 6011; no partial state; operator's co-signed initialize lands (payer ≠ governor); attacker-as-backend record refused, victim row never exists. lib.rs module/instruction docs now state the true guarantee (the old "no window" sentence is gone). Soak client step 4 + `docs/devnet-soak-m8.md`: initialize is now an ms-executed transaction (vault as `upgrade_authority` signer; historical 2026-11-01 evidence kept with a dated note).

**A-042 (med → fixed).** Every CPI-adjacent emitter reloads the mutated account before `emit!` (`vault.reload()?` ×2, `claim_mint.reload()?` ×2). Reload — not pre-state+delta — deliberately: the event must equal chain truth BY CONSTRUCTION; computed deltas assert an invariant they do not observe (a future fee-bearing USDC would falsify them silently). Events-section contract comment added: every `*_after` field is post-instruction truth. c2a/c2b/c2c flipped to the fresh values (0→5M, 5M→2M, 0→10M), each leg asserting mirror == truth on both sides.

**A-043 (low → accepted until M10; founder may override).** No code change per the director ruling. **M10 DESIGN CONSTRAINT (for the M10 kickoff): checkout MUST decide payer-identity binding for custodial-funded prepays** — books-side `prepaid.renter` attribution vs the paying `renter_key` — and the custodial-devnet flow (backend holds renter keys) must stay workable either way. `c3_prepay_can_credit_a_third_identitys_record` stays green as the standing pin of the accepted behavior (red-ready to flip the day binding is designed).

**Deps (step 10).** One dev-dep added: `solana-account =4.3.1` — genesis ProgramData override construction only; already inside the agave 4.1.2 lattice via solana-program-test (lock diff = exactly one edge, zero version moves — the A-033 lattice held; CI builds `--locked`).

**Verification.** `cargo-build-sbf --arch v3` (post-fmt ELF, the exact test subject) → pool crate **37/37** (4 unit + 20 adversarial + 4 journey + 9 audit — the flipped c1/c2 pins in CI permanently); `cargo fmt --all -- --check` clean; `clippy --workspace --all-targets -D warnings` clean; soak crate `cargo build --release` clean. Workspace dual-DB suite + push/CI verdict of the pushed head: in the DONE report (step 8). Devnet untouched.

**Fast jump — Session C fix touched:** `programs/pool/src/{lib,errors}.rs` (A-041 gate, A-042 reloads, docs) · `programs/pool/tests/common/mod.rs` (ProgramData override, loader const, deploy authority, initialize builder) · `programs/pool/tests/audit_session_c.rs` (c1/c2 flips + header) · `programs/pool/tests/{journey,adversarial}.rs` (fixture wiring only) · `programs/pool/Cargo.toml` + `Cargo.lock` (one dev-dep edge) · `scripts/devnet-soak/src/main.rs` + `docs/devnet-soak-m8.md` (ms-executed initialize) · `AUDIT.md` (A-041/A-042 fixed, A-043 accepted-until-M10) · `PLAN.md` (this entry).

### 2026-11-02 — M8 SESSION C: adversarial audit (step 14, fresh context) — 3 PoC-backed findings, 6 categories attempted-clean

**Persona & discipline.** Web3 smart-contract security auditor (director-named persona), fresh context, composition-first: the instruction alphabet × multi-instruction interleavings, every step individually legal. LOCAL rig only — SBF artifact rebuilt from HEAD (`cargo-build-sbf --arch v3` with the in-repo toolchain homes; the audit tests load those exact bytes); devnet untouched (soak evidence reused read-only). First chore executed: the director's dirty AGENTS.md step-14 amendment committed standalone (`fe8bdab`). Session C edited NO code and NO existing test; all findings route back through the user (step 14).

**Findings — AUDIT.md A-041…A-043, every one reproducible (`cargo test -p pool --test audit_session_c`, 9/9):**
- **A-041 (med, open) — permissionless `initialize`:** the first funded caller between deploy and the operator's initialize captures admin+backend for the whole deployment (D42 burns the program id for the operator); the captured backend then attests unbacked claims. lib.rs's "no window where a single dev key administers the pool" claim is falsified by PoC `c1`.
- **A-042 (med, open) — stale mirror events:** Anchor's deserialized-at-entry account copies are never reloaded after CPIs, so `PrepayRecorded/PrepayRefunded.vault_balance_after` and `ContributionRecorded.supply_after` emit PRE-instruction values (0 on the first record's supply). These events are M11's designed reconciler source and the A-039 detection layer — the mirror a subscriber rebuilds is wrong on EVERY event. PoCs `c2a/c2b/c2c` (simulate_transaction log capture + hand-rolled base64 event decode; no new deps).
- **A-043 (low, open) — unbound prepay identity:** the `renter` ARG is not bound to the paying `renter_key`; money can be prepaid into a third identity's record, which that identity can exit through the direct door. Conservation holds (no solvency break) — the harm is M10/M11 attribution drift. PoC `c3`.

**Attempted-clean (silence documented, step 14):** (1) *custody under composition* — deterministic 3-seed × 90-op sequence fuzz over the whole instruction alphabet mixing legal and illegal shapes (wrong doors, overdraws, u64::MAX edges, foreign destinations, mislabeled prepays, missing gate signers, pause/resume mid-flight, rotations-to-rogue): `vault == Σ prepaid` and `supply == Σ claims` held after EVERY op (`c7`); directed chains record→pause→burn→resume→refund, rotation-mid-sequence (no post-rotation window), rotation-cannot-imprison (`c6`). (2) *authority/governance* — admin `has_one`+Signer on every lever; admin rotation retires the old key instantly (`c5` leg d); backend rotation likewise (`c6` leg 2); single-key-upgrade refusal + 2-of-2 upgrade/pause/rotation are devnet-soak facts (read-only reuse; the BPF-loader upgrade path is not exercisable in program-test). (3) *claims & doors* — one shared `require_exit_authority`; direct-door refund overdraw refused (`c5` leg b); burn-while-paused refused on BOTH doors (`c5` leg a); zero refund refused (`c5` leg c); rule 9 proven at the TOKEN layer for the first time — a raw Token-2022 transfer of claims refuses with `TokenError::NonTransferable` (0x25) BEFORE any signature check, and the control pair (normal mint: signed transfer SUCCEEDS; NonTransferable mint: same signature REFUSED) isolates the extension as the refusing layer (`c4`). (4) *Token-2022 paths* — USDC side hard-pinned to Token v3 by Anchor owner checks; claim side pinned to Token2022 with mint/authority/token-program constraints; the NonTransferable+ImmutableOwner shapes were matched empirically against the program's own on-chain accounts (mint 170 B, row 174 B — measured, then mirrored; the doctrine suite never transfers claims, it only checks the extension flag). (5) *arithmetic* — all on-chain money math is checked ops; fuzz u64::MAX legs refuse cleanly. (6) *PDA substitution / D42* — every account seeds-bound under the program id, canonical bumps; the suite's two forgery shapes plus the fuzzer never produced a substitution.

**Suite-recursion pass (does the suite encode the doctrine?).** Strongly encodes: the door triad, both D40 mirrors per leg, pause-every-movement-class, forged-mint refusal, rule-9 flags, backend rotation. Gaps found — each was then attacked through the public surface in `audit_session_c.rs`, and the CODE held everywhere except where a finding is filed: initialize front-run never tested (A-041 live); renter/renter_key binding never tested (A-043 live); events never decoded or asserted anywhere (A-042 live); burn-while-paused untested (pinned `c5a`); direct-door refund overdraw untested (pinned `c5b`); zero-amount refund untested (pinned `c5c`); `set_admin_authority` never exercised in any suite (pinned `c5d`; the devnet soak did prove it live); no sequence/property fuzz existed (`c7` adds it).

**Observational (design notes, NOT findings).** (a) *reserved[64] cannot carry per-bucket positions (M9/D43):* it is 64 bytes on the SINGLETON while positions are per-depositor × per-bucket; claim rows are Token-2022 accounts whose layout the token program fixes (a single u64 balance — no custom fields ever). reserved[64] CAN carry a commitment: a 32-byte Merkle root of per-bucket positions + challenge-window parameters (exactly the A-039/M11 seam already anticipated); the positions themselves must stay in the books + Merkle leaves, or a new PDA family. (b) *Donation asymmetry:* anyone can SPL-transfer USDC directly into the vault PDA (token accounts accept inbound transfers), so externally `vault ≥ Σ prepaid`, never `==`; the mirror equality the suites assert holds only because the tests move money solely through the program. Direction is conservative (excess is stranded, not spendable) but M11's reconciler must compare `vault ≥ Σ prepaid` and treat excess as an alarm, not a given. (c) *Rent-lock:* claim rows and prepaid records have no close instruction — zero-balance accounts lock rent forever (backend-paid). Acceptable at M8 scale; should be a deliberate M9+ decision.

**Artifacts:** `programs/pool/tests/audit_session_c.rs` (NEW file, 9 tests — Session-C evidence; the three finding PoCs assert the bugs EXIST and go red when fixed, which is the handoff contract for the fix session) · `AUDIT.md` (A-041…A-043) · `PLAN.md` (this entry) · AGENTS.md chore `fe8bdab` (first chore). Deps: none added (hand-rolled base64 + raw wire builders; the lock lattice untouched). Local verification: pool crate 37/37 (4 unit + 20 adversarial + 4 journey + 9 audit), `cargo fmt --check` clean, `clippy --workspace --all-targets -D warnings` clean. Devnet: untouched, read-only.

**Fast jump — Session C touched:** `programs/pool/tests/audit_session_c.rs` (new) · `AUDIT.md` (A-041…A-043) · `PLAN.md` (this entry) · `AGENTS.md` (chore commit fe8bdab).

### 2026-11-01 — M8 devnet soak EXECUTED: all 10 steps green; done-when satisfied

**Unblocked by the founder** (faucet SOL for the deploy key; Circle browser faucet for the renter wallet — the API is retired, 404+reCAPTCHA). Final run evidence lives in `docs/devnet-soak-m8.md` (every sig) and `deploy/devnet-m8.json` (deployment facts). Highlights: program `9pEmyx…` deployed, upgrade authority = Squads vault `BhLQqw…`; **D40 proven live** (record_contribution minted claims with vault=0); **escape hatch proven live** (burn_claims with ONLY the depositor key in the transaction); renter prepay→refund round-trip exact through BOTH doors (vault==record==10 USDC → 0/0, wallet whole); single-key upgrade REFUSED; pause (error 6000) + resume + program upgrade + teammate rotation all executed as 2-of-2 vault transactions.

**The soak's real lesson — resilience engineering.** Eleven live bugs found and fixed DURING the run (all in tooling, none in the program — the on-chain code passed every step on first touch): unbound variable + dead Circle API in the orchestrator; a CLI-form the balance poll used that always read empty; `set -e` killing the script silently on one dropped RPC request; `write-buffer --buffer-authority` needing the KEYPAIR file not the pubkey; the Squads ProgramConfig treasury requirement (`require_keys_eq!` — treasury must be THEIRS, creation fee read from config); the ProgramData authority parser reading from-memory offsets and producing garbage (fixed by dumping the live bytes — never parse from memory); backend hot key having zero lamports on devnet; fee-payer/signer mismatches on direct-door and multisig legs; the buffer authority needing handover to the vault before a vault-signed upgrade; and the pool arriving PAUSED on restart (now resume-aware, which re-proves the resume lever). Each fix is commented at its site with the date.

**Bookkeeping:** SPEC §14 M8 ticked with the implementation pointer; docs evidence + deploy/devnet-m8.json filled; AUDIT A-040 records the parser-from-memory lesson (evidence integrity class). Zero V1 change; no new dependencies.

**Fast jump:** `docs/devnet-soak-m8.md` · `deploy/devnet-m8.json` · `scripts/devnet-soak/src/main.rs` (resilience fixes) · `scripts/devnet_soak_m8.sh` (orchestrator fixes) · `SPEC.md` (§14 tick) · `AUDIT.md` (A-040) · `PLAN.md` (this entry).

### 2026-11-01 — CI red on both M8 pushes: root cause fixed (SBF prerequisite), E2E flake hardened, D39 backfilled

**Root cause (director-diagnosed, confirmed in the Actions logs):** the Rust job's `cargo test --workspace` includes `programs/pool`, whose suites load the REAL SBF artifact (A-034 rig) — CI never built it, so all 20 program tests panicked on the missing-ELF assert. Local 200/200 was real; GitHub never built the test subject. Second issue: the first run's Dashboard E2E failure was a stack-boot flake (passed on rerun).

**Fixes.**
- **CI + SBF prerequisite — option (a), the rust job owns it.** Chosen over a separate "Program (SBF)" job because (workflow step 10 justification): it keeps ONE test verdict for the whole workspace — exactly the local semantics (`cargo-build-sbf --arch v3` from `programs/pool`, then `cargo test --workspace`), no job-split where the workspace run excludes pool and the program suite becomes a second-class citizen, and no duplicate workspace compile (the separate job would rebuild everything pool's dev-deps touch). No new cargo dependencies — the added "dependency" is the pinned anza release channel **v4.1.2** (the exact toolchain the A-033 lock lattice was built for; a newer CLI is not validated), cached under `~/.local/share/solana` + `~/.cache/solana` + `programs/pool/target`, keyed on `Cargo.lock`. CI's ELF uses an ephemeral program keypair (none is committed): the embedded address never enters execution — tests register the program at `declare_id!` and derive every PDA from it. Rule 6 honored: no skip-if-missing anywhere; the harness's hard ELF assert is what CI now satisfies, not bypasses.
- **E2E flake hardened** (`scripts/manual_journey_m4.py` + `scripts/e2e_j6.py`): the boot wait now requires TWO consecutive HTTP answers (a single answer can race still-initializing request state — the observed flake family), 60s→90s ceiling, and on boot-timeout the server log tail is dumped into the failure message. The E2E runner now ALWAYS keeps the throwaway server's log (`target/e2e-server.log`, gitignored) and prints its last 60 lines when Playwright fails — the next flake names itself.
- **D39 backfilled** in the SPEC D-log (gapless numbering restored; history kept honest — the ruling was folded into D38's rider at M7 GO; own anchor now, sweep ≤ accrued `equity.operator` recorded).
- AGENTS.md step-8 CI-verdict clause (director amendment, on disk) committed in the same commit.

**Verification:** local `cargo-build-sbf --arch v3` + `cargo test --workspace --locked` green (the exact CI sequence); `bash -n`/py_compile on the touched scripts; workflow YAML linted by push (Actions). CI verdict of the pushed head is in the session's DONE report per amended step 8.

**Fast jump — chores touched:** `.github/workflows/ci.yml` · `scripts/manual_journey_m4.py` · `scripts/e2e_j6.py` · `SPEC.md` (D39) · `PLAN.md` (this entry) · `AGENTS.md` (director's amendment).

### 2026-10-31 — M8 doctrine fix (director audit): D40/D41/D42 executed; program reshaped, suites rewritten

**Trigger.** Director doctrine audit (10 items + self-report) on suspicion of V2 drift. Verdicts: item 1 DRIFT (exit-without-cooperation missing — withdraw was backend-only), item 2 DRIFT (deposit moved USDC; pre-ruled D40), item 6 PARTIAL (renter-refund separability), all others ALIGNED. Self-report named both drifts before the fixes were ruled. Pre-filed rulings D40/D41 + new D42 recorded in SPEC §Appendix A same-commit as required.

**Executed fixes.**
- **D40:** `deposit` → `record_contribution` (mint-only; claims = micro-USD valuation of Path-B credits from the books; NO token transfer), `withdraw` → `burn_claims` (burn-only; no USDC out). Module docs rewritten around "why the vault never holds depositor principal."
- **Scope A (ruled):** minimal `prepay_renter`/`refund_renter` pair — the ONLY USDC path. Per-renter prepaid records as PDAs `[prepaid, renter]` (money-custody class, `RenterPrepaid { amount, bump }`); prepay increments vault + record; refund decrements both and is bounded by the renter's OWN on-chain record (`amount ≤ prepaid.amount`), never an operator attestation. This resolved audit item 6: separability is now on-chain fact.
- **D41 rider (two doors):** every exit (`burn_claims`, `refund_renter`) accepts backend (assisted, rule 10) OR the named identity's own key (direct, exit-without-cooperation), one shared `require_exit_authority`, identical ceilings on both doors. `record_contribution` stays backend-only deliberately: minting is an operator ATTESTATION from the books — a self-service mint would let anyone claim arbitrary contributions (A-039 documents the accepted caveat + containment).
- **D42:** no-cross-pool test added (`forged_claim_mint_from_foreign_deployment_refused` — two forgery shapes, both refused, real rows untouched).
- **Rule-8 guardrail (approved):** if the fee band ever goes on-chain it lands in `reserved` behind multisig AND still requires explicit user approval + D-log — a vote alone never changes the band (documented in state.rs + lib.rs).

**Test results.** Program: **28/28** (4 unit incl. new prepaid-layout tests, 20 adversarial — the director-ruled door triad first, refund-ceiling separability, both forgery shapes — and 4 journey: initialize-locked, contributions-are-records-not-money (vault stays 0 while claims grow — the D40 proof), renter round-trip through both refund doors, admin lifecycle pausing every movement class + backend rotation). fmt + clippy `-D warnings` clean (program + soak crate). Soak steps 5/6 rewritten renter-shaped (record + DIRECT-door burn; prepay → assisted + direct refunds) with a hard D40 violation check; orchestrator Circle-faucet section now targets the custodial user key as the renter wallet (≥10 USDC; depositor identity needs no USDC). Devnet soak still awaits founder faucet funding — unchanged blocker.

**Deps:** none added, none removed. **Zero V1 change:** `crates/` and `apps/` untouched.

**Fast jump — doctrine fix touched:** `programs/pool/src/{lib,state,errors}.rs` · `programs/pool/tests/{common/mod,journey,adversarial}.rs` · `scripts/devnet-soak/src/main.rs` · `scripts/devnet_soak_m8.sh` · `docs/devnet-soak-m8.md` · `deploy/devnet-m8.json` · `SPEC.md` (D40/D41/D42 + §14 annotation) · `AUDIT.md` (A-037…A-039) · `PLAN.md` (this entry).

### 2026-10-30 — M8 (local half): Solana escrow vault + claim registry — program, suites, soak client; devnet soak pending

**Scope (SPEC M8 row).** `programs/pool` Anchor program: escrow PDA custody of devnet USDC, deposit/withdraw of depositor principal, Token-2022 claim registry with `NonTransferable` (rule 9), 2-of-2 Squads multisig authority (devnet; production posture stays SPEC 2-of-3), program upgradable via multisig only, rule-10 discipline (no member/renter/depositor wallet), M7 seam mirroring. **NOT DONE YET: the devnet soak itself** — devnet airdrop was rate-limited, the founder took the faucet step ("i'll do it later"); everything up to that gate is done and the runbook is one command. **Zero V1 change:** `crates/` and `apps/` untouched; only `Cargo.toml` gained `programs/pool` as a member + a workspace release profile.

**Program** (`programs/pool/src/{lib,state,errors}.rs`, 7 instructions): `initialize` (singleton pool PDA, manual NonTransferable-mint CPI chain — anchor-spl has no non_transferable constraint — mint authority = pool PDA; vault token account owner = pool PDA), `deposit` (USDC source→vault + claim mint_to; `init_if_needed` claim row `[claim, depositor]` owned by the POOL; backend hot key pays rent), `withdraw` (burn claims + vault→destination; **backend signs ONLY** — rule 10; claims ≥ amount enforced), `pause`/`resume` (admin circuit breaker), `set_backend_authority`/`set_admin_authority` (multisig-gated rotation). `check_withdrawal_ceiling` = documented M11 no-op seam. Teaching comments throughout (founder learning Rust). Money is u64 micro-units; the 6-decimals seam is asserted at initialize AND in tests.

**Test rig — the hard-won part.** 16/16 green: 3 unit (state) + 10 adversarial (wrong signer/owner/mint/zero/overdraw/paused/non-admin/double-init/rule-9 lock/ownership) + 3 journey (initialize+registry locked; deposit×2/withdraw×2 round-trip with a vault↔claim mirror assertion after EVERY step; admin lifecycle). Two findings of note, both in AUDIT: **A-033** the solana dependency lattice has no floor — the lock is pinned crate-by-crate to agave v4.1.2's own Cargo.lock (wincode 0.5 cohort; `cargo update` WILL re-break; CI builds `--locked`); **A-034** anchor 1.2 + program-test 4.1.2 host-builtins are fundamentally incompatible (dual solana-sysvar stub statics) → tests load the REAL SBF artifact (`target/deploy/pool.so`) into the SBPF VM via `add_upgradeable_program_to_genesis` — the exact bytes that ship to devnet. Prerequisite: `cargo-build-sbf --arch v3` before `cargo test -p pool` (missing ELF = hard error, never a skip). Also: anchor instruction discriminators are namespaced `global:` (anchor-syn 1.2), accounts are consumed POSITIONALLY, and the rent sysvar is a declared account (host/SBF parity).

**Soak client** (`scripts/devnet-soak/`, standalone crate — its solana 1.18/squads cohort cannot live in the main workspace): steps 0-10 — USDC shape check, 2-of-2 Squads multisig create, authority→vault, single-key upgrade REFUSED proof, initialize, deposit/withdraw round-trip (25 USDC), pause via 2-of-2, paused-deposit refused, upgrade via buffer + multisig, member rotation. Built clean (`cargo build --release` ✓); byte-array consts self-checked at startup. Orchestrator `scripts/devnet_soak_m8.sh` (re-runnable, skips completed steps, non-interactive faucet polling). Runbook `docs/devnet-soak-m8.md`.

**Dependencies (every one justified).** anchor-lang/anchor-spl `=1.2.2` (framework; default v3 cohort — see A-033), solana-program-test `=4.1.2` + `agave-unstable-api` (in-process validator), granular solana dev-deps pinned to the agave 4.1.2 lattice (keypair/signer/pubkey/instruction/message/transaction/commitment-config/rent/system-interface/instruction-error/transaction-error), spl-token-2022-interface `=3.1.2` (extension-aware state PARSING only; bytes never cross cohorts), borsh `=1.8.1`, sha2 0.10 (discriminator), tokio. Standalone soak: squads-multisig `=2.1.0`, solana-client/sdk `=1.18.26`, serde_json, sha2, tokio. **A blind `cargo update` re-breaks the build** — the pins are load-bearing. `deny.toml` gained an `[advisories] ignore` list (11 RUSTSEC ids, each commented): every one enters ONLY via the program-test dev-dep chain (agave 4.1.2's own pinned tree — same versions agave ships); none compile into release binaries or the on-chain program; upgrading past them = leaving the agave lattice, a deliberate future decision, not drift.

**Test/evidence state.** Workspace: fmt + clippy `-D warnings` clean; `cargo test --workspace` 189 green (one SQLite-lock flake when two cargo jobs share the DB file — the property suite passes 2/2 in isolation; recorded here as the known parallel-invocation caveat); cargo-deny installed and queued for CI parity. Program: 16/16. Devnet: NOT RUN — awaiting founder faucet step; `deploy/devnet-m8.json` placeholders honestly read "FILLED AT SOAK".

**Resume (one command):** founder claims ≥4 SOL at faucet.solana.com for `4N2nTJ8WWwDKVoULt9bymaB6XsrB7HP973y8Ebw6tQhV` (deploy key), then `bash scripts/devnet_soak_m8.sh 2>&1 | tee target/soak-logs/soak-$(date +%Y%m%d-%H%M%S).log` — it deploys, faucets USDC (Circle API, browser fallback documented), runs the soak, verifies the post-soak authority is the Squads vault. Then: fill the evidence section of `docs/devnet-soak-m8.md`, tick §14.

**Fast jump — M8 touched:** `programs/pool/**` (src, tests/common, tests/{journey,adversarial}, Cargo.toml, Anchor.toml) · `scripts/devnet-soak/**` (crate + lockfile) · `scripts/devnet_soak_m8.sh` · `docs/devnet-soak-m8.md` · `deploy/devnet-m8.json` · `Cargo.toml` + `Cargo.lock` (member + lattice pins + release profile) · `.gitignore` (m8-keys, elfs) · `AUDIT.md` (A-033…A-036) · `PLAN.md` (this entry).

### 2026-10-28 — M7: LedgerTrait + double-entry ledger (V2 truth spine, ships dark)

**Scope (SPEC §6/§11, V2 M7 row + kickoff GO rulings):** the double-entry ledger, built but not open — no Solana, no deposits, no renters, no member surface, zero live request-behavior change. Chart of accounts + posting-rule-only creation + DB-enforced append-only + property-tested invariants + loud solvency.

**Kickoff rulings honored:** migration numbering kickoff said 0005 (stale) → director verified next is **0015**. "Budget→balance migration" ambiguity → **D38** (same commit, 8bbee09): preparation, never conversion — nothing mints balances from quotas; recorded in AUDIT A-030 per workflow step 9. Chart ambiguities routed back before any DDL: **Σ payable = custodial-cash debts only** (contributed principal → `claims.contributed`, outside rule 7, liquidity-buffer governed) and **operator fee → `equity.operator`** (not a payable) — both director-ruled at GO, recorded in D38 rider + AUDIT A-031.

**Schema (migrations/0015_ledger.sql):** `chart_of_accounts` (normative seed: 7 enum-coded accounts — cash.escrow, capacity.contributed, prepaid.renter, payable.depositor, claims.contributed, equity.operator, revenue.usage; typed columns + allowed-counterparties JSON, operator-perspective books), `ledger_accounts` (code→chart, owner_kind renter/depositor/org/operator, UNIQUE per owner+currency), `ledger_entries` (txn_id, seq, template_code, amount>0 CHECK, direction CHECK, ref, ts — the append-only truth), `ledger_account_balances` (signed Σdebits−Σcredits cache). House style kept: TEXT/BIGINT only, no DB-level FKs (SQLite would not enforce them without a per-connection pragma — referential integrity enforced inside the posting transaction).

**Append-only at DB level (rule 3):** sqlx runs every migration file on BOTH engines, so engine-specific guards cannot live in the .sql — they live in `run_ledger_guards` (crates/common/src/db.rs) hooked into `run_migrations`, the sanctioned home for DB-specific SQL. SQLite: BEFORE UPDATE/DELETE triggers with `RAISE(ABORT, 'ledger_entries is append-only')`, drop+recreated on every start (self-healing after any future table rebuild — structurally stronger than trusting migration authors to re-add them). Postgres: role split — migrations as owner; `tributary_app` NOLOGIN role holding exactly SELECT+INSERT on entries/accounts, SELECT on chart, SELECT+INSERT+UPDATE on the balances cache (a derivative, not truth); explicit REVOKEs kept as documentation (owners bypass grants; the split is the enforcement). No sequences → no USAGE grants needed (TEXT ids minted in Rust). Managed-PG users without CREATEROLE degrade to a loud startup warning, not a boot failure (zero-V1-behavior-change constraint; DBA can create the role by hand — deploy/README).

**Red-first burglar alarms (crates/ledger/tests/append_only.rs):** written BEFORE the guards, run RED (`[sqlite] direct UPDATE … SUCCEEDED — history is mutable, rule 3 is broken`; PG leg equally unguarded) — proof at `target/red-proof/m7-append-only-red.log` — then guards landed and the alarms went green on both engines: UPDATE and DELETE fail *for the right reason* ("append-only" / "permission denied"), INSERT as the app role still works (the alarm must not brick the door), seeded history provably intact. Runs in CI forever via `cargo test --workspace`.

**Crate (crates/ledger):** `MicroUsd` (i64 micro-units newtype, NO float conversions, crate-level `#![deny(clippy::float_arithmetic)]` — float-for-money is now a compile error); typed chart enums with strict parsing (drift = loud error, no fallbacks) + a hand-rolled strict counterpart-array parser (no serde dep); 6 posting-rule templates (the ONLY creation path — `PostingRequest` carries template+amount+owner slots, no leg list exists anywhere); `LedgerTrait` (explicit `impl Future + Send` — axum-ready now, Solana-swap later per SPEC §11) + `SqlLedger` over `common::Db`. Posting concurrency without row locks: cache updates are atomic SQL increments and every draw-down leg carries its sufficiency check inside the WHERE clause (`… AND -balance >= ?`) — race-free on both engines, no FOR UPDATE (which SQLite lacks). Trial balance, reconcile (cache vs entries + global), and solvency all recompute from entries — the cache is never trusted by the checks.

**Tests (16 new, all dual-DB):** journey (§11 story via PUBLIC trait only — fund→consume→deposit→settle fee+distribute→withdraw→solvency math→overdraw refused atomically; workflow step 12 honored, zero raw-SQL seeding), properties (3 seeded-LCG walks × both engines: per-txn Σdebit==Σcredit, trial balance closes, cache==shadow-model==entries, normal balances never wrong-side, rule 7 never violable by template flows; plus DB-chart==code-enum pin), alarms (tampered cache → DRIFT; bypassed lone-debit insert → TRIAL BALANCE VIOLATION; balanced escrow drain → SOLVENCY VIOLATION with exact numbers — proving the honest boundary: INSERT can lie, the detectors catch the lie), burglar alarms above. Ledger harness: **per-test isolated databases** (SQLite tempdir + throwaway Postgres DB per test — solvency sums are global; the shared-DB harness pattern would flake), PG-skip panics like its siblings.

**Bug found and fixed in-milestone (AUDIT A-032):** the M4 journey suite (shared SQLite DB, parallel boots) caught the guard DDL racing itself ("trigger already exists") because the statements ran as separate autocommits on a pool — fixed by running the whole guard set in one transaction (write lock serializes boots). This is exactly why the full workspace suite runs on every milestone: the ledger's own tests, all isolated, could never have caught it.

**Verification:** full workspace **172/172 dual-DB** (SQLite tempfiles + Docker Postgres 16 on :5433, container `tributary-pg-m7`, all 16 new tests included; M4 suite green with the race fix) · `cargo fmt --check` clean · `clippy --workspace --all-targets -D warnings` clean · `clippy -p tributary-gateway --features geoip -D warnings` clean · no `unsafe`. **Dependencies (workflow step 10): NONE added** — Cargo.lock diff is exactly the ledger crate's own edges (anyhow/sqlx/thiserror/tempfile/tokio/tributary-common, all pre-existing in-tree); serde deliberately NOT taken.

**Known limitations / honest notes:** (1) the binary still connects as the DB owner — the `tributary_app` flip is an M10 deployment step (role exists, grants CI-tested via SET ROLE); (2) guards bind the app connection, not the file owner/DBA — outer walls are perms/ops/audit + the ledger's own detectors (A-031); (3) SQLite single-connection `mode=memory` harness path (Windows interop) shares one DB across a test's dbs list by design; ledger tests use per-test isolation so unaffected; (4) local test residue: throwaway `ledger_test_*` databases inside `tributary-pg-m7` (container — `docker rm -f tributary-pg-m7` clears; CI containers are ephemeral); (5) demo/production containers (8111/8112) untouched, as ordered.

**Fast jump:** `migrations/0015_ledger.sql` · `crates/common/src/db.rs` · `crates/ledger/{Cargo.toml,src/{lib,money,chart,posting,ledger,error}.rs,tests/{harness/mod,append_only,journey,properties,alarms}.rs}` · `deploy/README.md` (ledger section) · `SPEC.md` (D38 + §14 tick) · `AUDIT.md` (A-030/031/032) · `PLAN.md` (this entry) · `Cargo.lock` (ledger edges only)

### 2026-10-27 — M6-S1-F1 FIXED + RELEASE v0.1.3 (director scope extension)

**Scope.** Director extension on top of slice 1: fix the minute-budget
defect slice 1 found (dashboard sent `limit_tokens` for the minute
"requests" window), pin the wire payload red-first, reword the two new
docs' workaround notes, release v0.1.3, smoke the released image, re-pin
the 8112 demo stack. Founder's 8111 left to pull at his convenience.

**Fix (`af7a79b`).** `AddBudgetDialog` (`apps/dashboard/app/org/pools/page.tsx`):
window → wire-field mapping — minute ("requests") sends `limit_requests`,
day/month send `limit_tokens` (teaching comment carries the v0.1.2 defect
story). The API already accepted `limit_requests`
(`UpsertQuotaRuleRequest`) — wiring only, zero gateway change.

**Red-first pin (`apps/dashboard/e2e/f1-budget-wire.spec.ts`, joined to
the `e2e_j6` journey run + a NEW CI e2e job).** Pins BOTH mappings —
minute → `limit_requests=N`/`limit_tokens=null` AND day → the inverse
(the full window→field mapping; neither half can regress) — plus the
enforcement meaning through the served dashboard: burst of 8 against a
minute/5 rule = exactly `[200×5, 429×3]`, 429 reads `Limit reached: 5
requests per minute`. RED observed pre-fix (wire carried
`limit_tokens=5, limit_requests=null` — the defect verbatim), GREEN
post-fix, J6 unaffected; minute-boundary flake guard (realign + retry)
in the burst leg. Local runs via `TRIBUTARY_BIN` (prebuilt v0.1.2
gateway — Rust untouched by this fix).

**Toolchain honesty.** `@playwright/test` 1.56.1 declared as
apps/dashboard devDependency (it was an UNDECLARED ad-hoc install in
node_modules; `e2e_j6` now prefers the workspace pin over `npx` — one
source of truth, no npx-cache dependency). New CI job `Dashboard E2E
(Playwright — J6 + F1 budget wire)`: first run GREEN on main
(run 40506644576) — the payload pin is CI-enforced.

**Docs.** Guide §budgets + runbook defect blockquote reworded: minute is
a true request cap since v0.1.3 (6th request in a minute 429s with
`Limit reached: N requests per minute`); pre-0.1.3 symptom retained for
old-image pilots (upgrade, or delete the rule + wait 5 s).

**Release v0.1.3 (`eb32ba8`, tag `v0.1.3`, release run 40507195511
success).** Bump set mirrors the v0.1.2 precedent: workspace Cargo.toml
+ Cargo.lock (5 crates) + apps/dashboard/package.json + README status +
landing eyebrow. CI on main green (Rust/dashboard/e2e/J7/deny). Multi-
arch GHCR: `0.1.3` + `latest`.

**Smoke on the RELEASED image.** Throwaway container on 0.1.3 (8113,
no-auth mock on a private network mirroring the m4-harness paths,
throwaway volume): healthz 200 · create-admin · the repo F1 spec run
against the container's served dashboard: minute →
`limit_requests=5/limit_tokens=null` ✓, day → inverse ✓, burst
`[200×5, 429×3]` ✓, "Limit reached: 5 requests per minute" ✓. Rig torn
down after.

**Demo re-pin (8112).** `tributary-demo` recreated on 0.1.3, SAME
volume + master key + port + dual-homing (bridge + demo-net@172.28.0.3)
+ restart policy — stories and usage parity intact by construction;
healthz 200, dashboard serving. Founder's 8111 untouched (`latest`,
pulls when he likes: `docker compose pull && docker compose up -d`).

**Fast jump — F1 + v0.1.3 touched:** `apps/dashboard/app/org/pools/page.tsx`
· `apps/dashboard/e2e/f1-budget-wire.spec.ts` (new) · `scripts/e2e_j6.py`
· `apps/dashboard/package.json` + `pnpm-lock.yaml` (@playwright/test
declared) · `.github/workflows/ci.yml` (e2e job) ·
`docs/onboarding-org-admin.md` + `docs/pilot-support.md` (defect notes) ·
version bump set (Cargo.toml/Cargo.lock/package.json/README/landing) ·
commits `b3857b4`/`af7a79b`/`eb32ba8` + tag `v0.1.3` · GHCR
`0.1.3`/`latest` · demo stack re-pinned in place.

### 2026-10-27 — M6 SLICE 1: pilot onboarding surface (evidence-independent half)

**Scope.** Four docs, zero code (kickoff: dashboard friction = finding,
never a patch): pilot feedback issue template, image-first upgrade rung
in deploy/README, founder diagnostics runbook, admin first-day guide
(DRAFT v1). Everything verified against a LIVE v0.1.2 GHCR image on
throwaway stacks (ports 8113/8116, own volumes; founder 8111 + demo 8112
untouched; local-registry stand-in for the drift proof; mock upstream =
`scripts/mock_openai.py` in a container — repo contract-test method,
disclosed in the guide's DRAFT banner per director approval).

**Deliverables.** `.github/ISSUE_TEMPLATE/pilot-feedback.yml` (version +
rung + worked/broke[steps+logs]/missing/keep-using; linked from README
"Where to go next") · `deploy/README.md` **Upgrading** rewritten (pull
REQUIRED — `pull_policy: missing` means plain `up -d` never consults the
registry, proven live against a local registry; tag-pin tradeoff, pin
recommended for pilots; worked example v0.1.0→v0.1.2; one-line rollback
with the honest migration caveat — 0.1.0↔0.1.2 schemas are identical
(verified: `git diff v0.1.0 v0.1.2 -- migrations/` empty), which is why
that round trip is clean; volume/.env/master key untouched) ·
`docs/pilot-support.md` (phone runbook: what-version probe (`docker
compose images` — no `--version` flag exists) → healthz meanings →
`docker logs` listening guide → audit log → review queue → fixes #1–#5:
port conflict, docker-group, stale image, `store:false` on pre-D37
images, WSL/two-engines) · `docs/onboarding-org-admin.md` (host→org
admin handoff, member→pool→provider key (validate-on-save)→key shown
once→budget→first request→usage→review-queue orientation; DRAFT v1,
rewritten in slice 2 from pilot friction).

**Journey verification (live 0.1.2, Playwright against the served
dashboard, full journey green, 32.3 s).** Empty box → host login → org →
mint org admin → org-admin login → member → pool (allowlist m-test) →
provider key (Custom → mock, validated `ok` at save) → issue key (raw
captured from the network response AND the reveal toggle) → day budget
100k → first request **200 "Hello!"** → burst under a 300-token cap =
`[200,200,429,429,429,429,429,429]` matching the reserve/settle
arithmetic exactly → mock upstream hit accounting proves **every denial
was pre-upstream** (hits = successes only — rule 4 at work). Screenshots
at /tmp/m6s1/shots (session artifact, not committed).

**Finding M6-S1-F1 (open — dashboard defect, NOT patched per kickoff
rule).** The *Set budget* dialog's "Minute (requests)" window sends
`limit_tokens` (never `limit_requests`) — wire-verified on 0.1.2:
`{"period":"minute","limit_tokens":5,"limit_requests":null}` — so "5
requests/minute" creates a 5-TOKEN-per-minute cap that denies **every**
request at reserve time (live 429: "Estimated 258 tokens … exceeds the
remaining budget of the 5-token minute window"), and the rule list even
displays it back as "5 tokens". Recovery: delete the rule, wait ~5 s
(RULES_TTL — below), member works again (verified). Cause in
`apps/dashboard/app/org/pools/page.tsx` `AddBudgetDialog`; API accepts
`limit_requests` (`UpsertQuotaRuleRequest`), so the fix is a dashboard
one-liner + follow-up test — left for a code slice with director
blessing. Docs handle it honestly: guide §budgets says don't use the
minute window; runbook fix-table covers the all-429 symptom.

**Non-finding, documented.** Quota-rule changes take ≤5 s to reach
gateway admission (`RULES_TTL`, quota.rs, D9 in-proc cache) — my first
"still 429 after delete" detour was exactly this, confirmed against
source + live (fresh key 200s after the TTL passes). Now stated in both
new docs. Also confirmed NOT provider rate-limiting: mock never
rate-limits and denied requests never reached it.

**Residue.** Spec §14 M6 stays open (done-when = first real org
daily-driving); CI untouched; no SPEC change. Slice 2 = rewrite the
guide + feedback loop from the founder's pilot-1 friction log (parallel
lane, not this session). Scratch Playwright spec lived under
`target/m6s1/` (gitignored, removed post-verification).

**Fast jump — M6 slice 1 touched:** `.github/ISSUE_TEMPLATE/pilot-feedback.yml`
(new) · `deploy/README.md` (Upgrading rewrite + pilot-docs pointer) ·
`docs/pilot-support.md` (new) · `docs/onboarding-org-admin.md` (new) ·
`README.md` (Where-to-go-next rows) · `PLAN.md` (this entry + status).

### 2026-10-27 — D37 + RELEASE v0.1.2 (director consolidated instructions)

**Scope.** Director ruling D37 (relayed by the founder after the live
`store` bisect): implement the negation-only field drop, release it as
v0.1.2 (any image without D37 400s the founder's harness dogfood),
smoke the released image incl. the drop behavior, re-pin the 8112 demo
stack (the Nov-12 video records on it), fill the VERIFIED slot. E9 and
the founder's REAL-CONSUMER appendix deliberately NOT chased (E9 moves
to the on-camera staging on Nov 12; the appendix runs the moment
v0.1.2 lands on his 8111).

**Implementation (`709a340`).** `chat.rs`: `PROVIDER_FIELD_DROPS`
explicit per-kind allowlist (`[("gemini", "store")]`, budget ≤ 3/kind
forever) + `apply_provider_field_drops` — negation-only (literal
`false` removed; truthy `store` NEVER dropped, its upstream 400
propagates as the honest signal); applied after key-selection decrypt,
before the stream_options injection. Teaching comments carry the
doctrine: a compat affordance, not format conversion; no generic
unknown-field stripping. Red-first dual-DB in `m55_gemini.rs`: the mock
now mimics Google's store-field 400 byte-for-byte (array-wrapped
INVALID_ARGUMENT, captured live); three permanent tests — (a)
store:false → gemini pool → 200 + metering row intact, (b) store:true →
400 propagates + metered at zero tokens, (c) PERMANENT non-interference:
openai-labeled key (same compat URL — the LABEL drives the drop)
forwards letter-for-letter, asserted byte-exactly from the mock's
captured hit. Red observed on (a) pre-impl; 7/7 green after; fmt/clippy
clean; full workspace suite green (SQLite + Postgres, panic-not-skip).

**SPEC D37 entry** added in the same commit; **deploy/README** gains
the "Field drops (D37)" paragraph (what/why, negation-only, honest-400,
letter-for-letter for other labels) and the **VERIFIED slot**:
`gemini-2.5-flash`, live 2026-10-27 (`be0a16d`).

**Release v0.1.2 (`328dd89`, tag `v0.1.2`, run 40464087947
success).** Bump in workspace `Cargo.toml`/`Cargo.lock` (5 crates)/
`apps/dashboard/package.json`/README status/landing eyebrow. CI on
main: green. Multi-arch GHCR: `0.1.2` + `latest`
(digest `sha256:58f39f21…`, matches the smoke-tested image).

**Smoke on the RELEASED image** (`/tmp/smoke/`, compose-free, 8115 +
mock on a private network, throwaway volume): healthz 200 ·
create-admin · login · public-API provisioning (gemini-labeled +
openai-labeled keys against a Google-faithful store-rejecting mock) ·
A) gemini store:false → **200**, mock saw the field ABSENT (the drop
happened) · B) gemini store:true → **400** propagated, mock saw `true`
· C) openai store:false → **400**, mock saw `false` letter-for-letter.
All assertions green; stack torn down.

**Demo re-pin (8112).** `tributary-demo` recreated on `0.1.2`, SAME
volume + master key — stories intact (classifier + D32 flags), usage
parity intact (281 / 15 days / 460,492 tokens), dual-homing restored
(demo-net 172.28.0.3), slice-2 UI in the served bundle.

**PENDING DIRECTOR QUESTION (founder, 2026-10-27): LIVE demo, no video.**
The hackathon demo will be performed LIVE on Demo Day (Nov 14), not a
recorded video shown that day. REAL-CONSUMER appendix items 1–3 are DONE
with evidence (usage rows, key row, chat panel); item 4 (save the
recording) is skipped by this decision. Needs director re-scope:
(a) **E9's new home** — was parked at "the Nov-12 video recording";
candidates: a pre-event rehearsal, or ON-STAGE (phone on mobile data +
ts.net + same member key = the strongest live version);
(b) **stale commitments** — SPEC §14 M5.5 residue ("video records
~2026-11-12, README link same day") and D36 ("demo video doubles as
Demo Day material") assumed a recording;
(c) **script doc role** — `docs/demo-video-script.md` becomes the LIVE
script; a live performance needs a rehearsal slot, per-scene plan B
(venue Wi-Fi vs tailnet; phone hotspot as fallback network AND the E9
prop), and demo-stack resilience on the day (8112 stack already
re-pinned to 0.1.2 and stable).

**Fast jump — D37 + release touched:** `crates/gateway/src/chat.rs`
(drop fn + allowlist const) · `crates/gateway/tests/m55_gemini.rs`
(store-rejecting mock + 3 permanent tests) · `SPEC.md` (D37 row) ·
`deploy/README.md` (D37 paragraph, VERIFIED slot) · version bump set ·
commits `709a340`, `be0a16d`, `328dd89` + tag `v0.1.2` · GHCR
`0.1.2`/`latest` · demo stack re-pinned in place.


### 2026-10-27 — CHORE: v0.1.1 image release + demo re-pin (M5.5, director order)

**Scope.** Director image chore (blocking the ~Nov-12 video): the slice-2
UI existed only in `main`, not in any released image. Bump/tag → smoke →
re-pin the 8112 demo stack → PLAN entry; founder then re-pulls 8111.

**Release.** Version bumped 0.1.0→0.1.1 in `Cargo.toml` (workspace) +
`apps/dashboard/package.json` + README status line + landing eyebrow
(Pages redeployed automatically, green). Director's SPEC housekeeping
(M6 checkbox wording: adoption focus, paid tier void per D35) swept
into the release commit `65ce698`. Tag `v0.1.1` pushed → release
workflow run 40444812015 **success** (~46 min, multi-arch amd64+arm64 →
GHCR tags `0.1.1` + `latest` — the semver pattern drops the `v`, M5
convention). CI on `main` at `65ce698`: **green** (Rust dual-DB incl.
m55_gemini, cargo-deny, dashboard, SDK contract incl. the new
Gemini-compat legs). Landing Pages run green. Zero migration changes
v0.1.0→v0.1.1 — volumes carry over by construction.

**Smoke (8113, compose-free, throwaway volume):** healthz 200 ·
create-admin ok · login ok · provider-select UI ("Gemini (AI Studio
key)") present in the served static bundle. Removed after.

**Demo re-pin (8112):** `tributary-demo` recreated on `0.1.1` with the
SAME volume + SAME master key (recovered from the old container's env)
— all staged state survived: healthz 200, avery login ok, review queue
= the two stories (classifier + D32), usage parity intact (281 rows /
15 days / 460,492 tokens), dual-homing restored on demo-net
(172.28.0.3), slice-2 UI found in the bundle. `/tmp/repin_demo_v011.sh`
(throwaway, not committed).

**REAL-CONSUMER finding (founder dogfood, DSH → pool → real Gemini,
2026-10-27 ~10:40):** DSH's first turn 400s; DB evidence shows the pool
behaved perfectly — a burst of 7 `security.quota_denied` audits while
the 10-token cap lived (denied BEFORE upstream, no Google cost), then
after cap removal three requests ADMITTED, ROUTED, and METERED with
`status=400, 0 tokens`. Diagnosis via a throwaway capture proxy
(/tmp/dsh_capture_proxy.py → :8114): **Google's compat layer rejects
`"store": false` — a legitimate OpenAI param — with 400 "Unknown name
'store': Cannot find field"** instead of ignoring it ("OpenAI-compatible"
≠ OpenAI-complete). Plain messages, a simple tool schema, and streaming
all pass through the pool (three green bisect curls). Tributary needs
no change (router, not converter — the posture held); **auto-bisect
verdict (founder-run, 2026-10-27): "200 OK after dropping: ['store']" —
`store` is the ONLY offender; DSH's full agent payload incl. tools,
streaming, and `max_completion_tokens` passes otherwise.** Fix is
DSH-side field suppression (or a store-strip shim IF the director rules
one in — it brushes the no-converter doctrine); Demo Day pairings that
work today: official OpenAI SDK clients → pool → Gemini (J-D-proven)
and DSH → pool → OpenAI. **DSH feedback note CLOSED (director,
2026-10-27):** D37's negation-only `store:false` drop resolves the
Gemini path Tributary-side — DSH → pool → Gemini now works with zero
DSH changes. The two external feedback items handed to DSH, recorded
for the record: (a) `store` should be client-configurable, not sent
unconditionally (it is the sole offender against Google's compat
endpoint — "200 OK after dropping: ['store']" in the 39KB bisect);
(b) thought-tag rendering request (dogfood UX). Nothing further
pending on the Tributary side.

**FOUNDER NEXT ACTIONS (image chore):**
1. `docker compose pull && docker compose up -d` on your 8111 instance
   (volume + master key carry over; zero migrations). E1–E8 unchanged;
   the E3 Gemini dropdown + provider column now render (verify E3's
   label check).
2. Run `/tmp/replay_bisect.py 'sk-pool-…'` → send the "200 OK after
   dropping: […]" line for the record.
3. E9 + the harness dogfood against 8111 on the NEW image (pull first);
   record the dogfood session — Demo-Day footage.

**Fast jump — chore touched:** `Cargo.toml`/`Cargo.lock` ·
`apps/dashboard/package.json` · `README.md` (version) ·
`landing/index.html` (eyebrow) · `SPEC.md` (director M6/D35 sweep) ·
`PLAN.md` (this entry) · commit `65ce698` + tag `v0.1.1` · GHCR
`ghcr.io/phat1911/tributary:0.1.1` / `:latest` · demo stack re-pinned
in place (no repo files).


### 2026-10-27 — M5.5 SLICE 2 (FINAL): Gemini provider support — CLOSES M5.5

**Scope.** Director kickoff (GO + 4 additions: rate-limit honesty,
allowlist string-match verification, REAL-CONSUMER appendix, FOUNDER
NEXT ACTIONS block). D36's honest Google-tech requirement: a real
Gemini key as a first-class pool provider via the OpenAI-COMPATIBLE
endpoint (`generativelanguage.googleapis.com/v1beta/openai/`), merged +
verified + demoable by ~2026-11-10 (video ~Nov 12, Demo Day Nov 14).
Journeys define done (workflow step 12): J-A provision, J-B serve, J-C
govern, J-D contract (CI, mock upstream, no real key), J-E founder
real-key manual script.

**The headline finding (confirmed director suspicion (a)):** NO
gateway-code change was needed. Migration 0004 already gave
`provider_keys` a `provider` label and an API-settable `base_url`; the
gateway already routes `{base_url}/chat/completions` with
`Authorization: Bearer` (chat.rs:314) — exactly the compat endpoint's
auth shape. Provider-agnosticism was PROVEN, not assumed: the J-B
journey pins Bearer-auth + no-query-string on every upstream hit, metering
row + IP fingerprint parity, and identical quota arithmetic (same 11/7
token counts as the OpenAI mock). Allowlist matching verified as PLAIN
string equality (chat.rs:76 — no gpt-* shape assumptions; addition #2
satisfied, and the gemini allowlist test proves it end-to-end). The
teaching pin for Bearer-vs-`?key=` lives in the test mock itself: any
request with a query string is 401'd, so a future regression to
query-param auth turns the whole suite red.

**J-A green first (director's order), then breadth.** All four journey
tests green dual-DB (SQLite tempfile + Postgres 16 on :5433, throwaway
`tributary-pg-m55`): `j_a` provisioning (encrypted at rest, masked
everywhere, provider label round-trips, compat base_url stored
verbatim), `j_b` serving (non-stream + stream legs — injected usage
chunk stripped downstream, both usage rows on the gemini provider key
with model pass-through, IP fingerprint row), `j_c_quota` (day-cap 429
BEFORE upstream + exactly one `security.quota_denied` audit row), `j_c_d32`
(two-/24 sharing flag fires alert-only on gemini-routed traffic,
deduped to exactly one). J-D extended `contract_openai_sdk.py`: the
two SDK legs are parameterized by model, and two new legs run the
official Python + JS OpenAI SDKs against a mock Gemini-compat upstream
(in-process `GeminiCompatMock`, same Bearer/query-string pins) — 4/4
legs green locally; CI's `sdk-contract` job runs the same script, no
workflow change.

**UI (minimal, not a redesign).** Add-provider-key dialog gains a
provider select (OpenAI / Gemini (AI Studio key) / Custom
(OpenAI-compatible)) that prefills the base URL and key placeholder,
sends `provider` with the create call; the key list gains a Provider
column. tsc + eslint + next build clean.

**Docs + landing.** README: provider-keys paragraph (works with OpenAI
and Gemini keys, any OpenAI-compatible endpoint; NOT a format
converter; free-tier rate-limit honesty). deploy/README: full
"Provider keys — OpenAI and Gemini" section — provider table, honest
boundaries, the model-ID enumeration curl (Bearer, never `?key=`),
plain-string allowlist note, a **VERIFIED-on-this-deployment slot** the
founder fills at J-E (no blind hardcoding — director confirmation (b)).
.env.example: TRIBUTARY_MODELS gemini example. Landing hero lede:
"OpenAI and Gemini keys both work, along with any OpenAI-compatible
endpoint" — posture grep clean (sole match remains the rule comment
itself).

**J-E (founder real-key script).** `docs/founder-journey-m55-slice2.md`:
E1 model-ID enumeration curl (record the id — fills deploy/README's
VERIFIED slot) → E2 pool with gemini allowlist → E3 add Gemini key
through the NEW UI field → E4 member curl → E5 real answer → E6 usage
row → E7-E9 quota 429 + D32 leg. Rate-limit honesty box up top (must
not meet 429s first on stage Nov 14). REAL-CONSUMER appendix: repoint
any OpenAI-compatible harness at `http://localhost:8111/v1` with a
member key, record the session — Demo-Day footage (after the tick;
J-E itself stays curl).

**Demo stack re-stage + parity re-seed (slice-1 open item).** Fresh
stage per the video script's pre-stage, collision-proof: `tributary-demo`
@ **8112**, volume `tributary-demo-data`, dual-homed on recreated
`demo-net` (172.28.0.0/16, gateway at 172.28.0.3), `demo-upstream`
mock, `seed_post_demo.py --base http://localhost:8112` green end-to-end
(quota 429 / D32 / classifier / rotation stories), shadow-leg
classifier_flag dismissed via the public resolve API (queue = the two
stories). Usage-history parity re-applied through the sanctioned
exception — this time WITHOUT host sudo: a throwaway helper container
(never committed, chore precedent) edited the stopped volume in place,
backing up the db+wal+shm trio beside it and chown-ing everything back
to 999:999 (both 2026-10-25 lessons honored). Result: 281 rows / 15
days / 460K tokens — pool pages show lived-in numbers (verified via
the public usage/summary API). Master key lives in the container env
(`docker inspect tributary-demo` recovers it if the container must be
recreated against the same volume; fresh re-stage = fresh key + fresh
volume per the video script). Stack LEFT RUNNING (unless-stopped) for
the ~Nov-12 record; the founder's own `tributary` @8111 untouched.

**Video script updates.** Pre-stage re-pointed to the collision-proof
names (tributary-demo @8112, seed `--base`, master-key recovery note) —
the script still taught the pre-incident layout, which would have
collided on this laptop. Scene 1 keeps 8111 (his own quickstart
container IS the scene); browser scenes 2–8 now say 8112. New
"Demo Day addendum — the Gemini scene": G1 enumeration curl → G2
add-key UI → G3 member curl + usage row → G4 harness dogfood on his
own instance (keep a 429 on camera if one comes — honesty is the demo).

**Step-10 (deps):** none. No Cargo/pnpm/Actions changes (the contract
job runs the same script; `cargo test --workspace` picks up the new
test binary). `scripts/__pycache__/` added to .gitignore.

**Tests/verification.** Workspace `cargo test` green dual-DB (incl.
m55_gemini 4/4); `cargo fmt` clean; `clippy --workspace --all-targets
-D warnings` + geoip variant clean; dashboard tsc/eslint/build clean;
contract 4/4 legs green locally (CI re-runs); landing posture grep
clean. No AUDIT.md entry — no new finding (no gateway-path change, no
new deps, no secret-handling change; the `?key=` pin is a test, not a
fix).

**FOUNDER NEXT ACTIONS (chased at the Nov-7 director checkpoint):**
1. **Obtain a Google AI Studio key** — <https://aistudio.google.com/apikey>
   (free) — and finish the deploy-journey redo if not yet done (your
   own instance on 8111 is the J-E target).
2. **Run J-E** (`docs/founder-journey-m55-slice2.md`, 10–15 min) with
   the real key; send the filled report; fill deploy/README's VERIFIED
   slot with the E1 model id and commit it.
3. **After the tick:** repoint your AI harness at the pool
   (`http://localhost:8111/v1` + a member key) and **RECORD the
   dogfood session** — that footage is a demo-video scene (Demo Day,
   Nov 14). Then the full video records ~Nov 12 per
   `docs/demo-video-script.md` (demo stack already live on 8112), and
   the README link lands same day (M5.5's last residue).

**Fast jump — slice 2 touched:** `crates/gateway/tests/m55_gemini.rs`
(new) · `scripts/contract_openai_sdk.py` ·
`apps/dashboard/app/org/pools/page.tsx` · `README.md` ·
`deploy/README.md` · `deploy/.env.example` · `landing/index.html` ·
`docs/founder-journey-m55-slice2.md` (new) · `docs/demo-video-script.md`
· `SPEC.md` (§14 M5.5 tick) · `PLAN.md` (this entry) · `.gitignore` ·
commits d258b1f / 0d0c712 / 9984f46 / 5367101 (+ this close-out)


### 2026-10-26 — M5.5 SLICE 1: landing page + Tailscale rung + demo-video script

**Scope.** Director kickoff (Gemini EXCLUDED — separate slice per D36
timeline): (1) static landing page, plain HTML+CSS committed at
`landing/`, served by GitHub Pages at the free github.io URL (no
custom domain, no new service, zero recurring cost), D35 "everything
free" posture (no pricing/tiers/managed/support/waitlist wording), the
2-minute compose quickstart, AUDIT/come-break-it + repo/docs CTAs, one
V2 line, public screenshots generated by the in-repo rig (staged demo
data, masked keys only, no private-vault references); (2) deploy
ladder rung 3 (Tailscale) in `deploy/README.md` — localhost(1) →
LAN(2) → tailnet(3) → public VPS(4); (3) `docs/demo-video-script.md`
(scene-by-scene, 2½ min, exact pre-staged UI states, non-native-readable
narration) + `docs/founder-journey-m55-slice1.md` (step-12 manual
journey: every landing CTA + Tailscale rung headings). Two GO additions:
bidirectional README→landing link; og/twitter share-preview tags
(og:image = staged S1 shot at the absolute github.io URL). No SPEC
change, no §14 tick (M5.5 checkbox stays open for the Gemini slice).

**Staging facts.** The chore-time stack was gone (containers + volumes
removed) — full re-stage from `ghcr.io/phat1911/tributary:latest`,
`scripts/seed_post_demo.py` green end-to-end. **New lesson (the D32
story collapses otherwise):** `demo-net` must exist with
`--subnet 172.28.0.0/16`, and the gateway must be DUAL-HOMED — default
bridge (published 8111, traffic sources from 172.17.0.1) plus a demo-net
interface at 172.28.0.3; a demo-net-only container SNATs published-port
traffic to 172.28.0.1, putting BOTH legs in one /24 (no flag — first
attempt failed exactly there). Shadow-leg `classifier_flag` dismissed
via the public resolve API (queue = the two stories, chore precedent);
270 usage-history rows added through the sanctioned exception using the
two logged lessons (copy db+wal+shm together; chown 999 after
copy-back) — offline script, intentionally NOT committed (chore
precedent).

**Files.** `landing/{index.html,style.css,assets/}` (4 PNGs @2x from
the rig + favicon.svg) · new `.github/workflows/pages.yml` ·
`deploy/README.md` (sharing-ladder section, rungs 1–4) · new
`docs/demo-video-script.md` · new `docs/founder-journey-m55-slice1.md`
· `README.md` (one landing line under the tagline) · rig extension:
`apps/dashboard/e2e/post_shots.spec.ts` (+S0 org home, +S3 pool
workbench — navigation by link-click, never hardcoded ids) +
`scripts/post_shots.py` (docstring) · `PLAN.md` (this entry).

**Step-10 justification (new Actions usage).** The Pages workflow uses
only GitHub FIRST-PARTY actions, pinned to current stable majors:
`actions/checkout@v5` (same major as ci.yml/release.yml),
`actions/configure-pages@v5`, `actions/upload-pages-artifact@v3`,
`actions/deploy-pages@v4`. No third-party code, no runtime dependency,
no Cargo/pnpm changes (cargo-deny untouched). Cost: $0 (Pages free
tier, public repo). Ops note: repo Settings → Pages must have source
"GitHub Actions" (one-time founder UI flip; flagged in the journey
script's pre-flight).

**Journey findings, fixed live:** #1 (2026-10-26, founder at rung 3
step 3 on a WSL host): `ufw: command not found` — the step assumed a
firewall tool exists. Step 3 now leads with `sudo ufw status` and the
"command not found / inactive → skip (never install one just for
this)" outcome. #2 (2026-10-26, founder at rung 3 step 6 on WSL):
`ERR_CONNECTION_TIMED_OUT` from the Windows browser — Tailscale lived
only inside WSL, so Windows had no route to the 100.x address. Rung 3
step 6 now carries a WSL note (install the Windows app on the same
account; two nodes for one laptop is normal; plus the in-WSL healthz
sanity check). Host-side verified good during diagnosis: HTTP 200
over the tailnet IP, 0.0.0.0:8111 binding. #3 (2026-10-26, founder
at rung 3 step 6, WSL + Docker Desktop): from the Windows browser the
WSL node's own tailnet address hangs while the WINDOWS node's address
serves (Docker Desktop republishes the port on Windows); through that
door every member arrives as the same proxy address, so IP-derived
controls cannot distinguish members. WSL note extended (healthz
decides which address; Linux-host/VPS is the honest topology for real
member groups). Founder login confusion resolved inline: the staged
demo stack answers only to its seeded throwaways (avery@labs.example /
AveryLaunch-Demo2026, host@tributary.example / HostLaunch-Demo2026),
not to his stranger-run volume's accounts. Root cause of the confusion
(finding #3b): the laptop runs TWO docker engines — native dockerd in
WSL (the staged demo, localhost:8111) and Docker Desktop on Windows
(his stranger-run deployment, which is what the Windows tailnet
address 100.64.11.1:8111 actually serves). Verified: identical
credentials 200 at localhost, 401 at the tailnet door. His own
deployment over the tailnet doubles as the honest Part-C journey
proof. The bootstrap-once `create-admin` refusal he hit is by design
(first admin only; further users via the admin API). **Post-journey
incident (2026-10-26 ~16:00):** resetting his throwaway, the founder
ran the PowerShell-syntax commands inside WSL bash and `docker rm -f
tributary` removed the tailnet-door instance AND collided with the
staged demo's container name on the now-single effective engine
(laptop topology: Docker Desktop WSL integration over a native
dockerd; socket ownership flipped at 15:18 local). Recovery: demo
re-staged collision-proof as `tributary-demo` on host port **8112**
(`localhost:8112`, seed + shadow-flag dismissal re-run, stories
green); usage-history parity NOT re-applied after the wipe (the
sanctioned-exception script is throwaway; re-run before the Nov-12
video if the pool page should show lived-in numbers — IDs in the
/tmp script need refreshing first). His own fresh instance takes the
name `tributary` / port 8111 (the tailnet door) — no further name or
port collisions possible. Lesson recorded: laptop runs multiple
docker engines; container names are per-engine but published ports
are per-machine. **Finding #4 (2026-10-26, founder login loop at
rung 3 step 6):** the session cookie is `HttpOnly; Secure;
SameSite=Lax` (correct hardening), and browsers refuse Secure cookies
on plain http:// except localhost — so DASHBOARD login over
`http://100.x.y.z:8111` silently loops back to /login. API-key
traffic (`/v1/*`) is unaffected (no cookie involved). Rung 3 step 6
rewritten: browse from localhost, or `sudo tailscale serve --bg
http://localhost:8111` for automatic tailnet TLS
(`https://<machine>.<tailnet>.ts.net`) — the honest browser path
over the tailnet. His instance verified healthy through the loop
(door healthz 200; dummy login clean 401) — client-side cookie
storage was the whole story. **Finding #5 (2026-10-26, tailscale serve
on WSL):** `tailscale serve --bg` configured cleanly and the ts.net
URL answers 200 from inside WSL — but the Windows peer cannot reach
the WSL node's tailnet IP at all (same blocked leg as finding #3;
Tailscale-in-WSL + Tailscale-on-Windows simultaneously is a
known-messy, docs-warned combo). Practical close-out: founder logs in
at localhost (Secure-cookie exempt) — Part C satisfied by the instance
serving over the tailnet + login from the host. Recommended clean
topology for this laptop, recorded for the video re-stage: ONE
Tailscale, Windows only (remove the WSL one + its node from the admin
console), then `tailscale serve` on Windows against the WSL-forwarded
localhost:8111. **Teardown (2026-10-26 ~16:40):** founder chose a
clean-machine redo of the deploy journey; all session resources
removed (containers tributary/tributary-demo/demo-upstream, volumes
tributary-data/tributary-demo-data/tributary_tributary-data, network
demo-net, /tmp staging files). Stack-down note: the ~Nov-12 video
re-stage starts fresh per docs/demo-video-script.md pre-stage. The
landing A-part of the journey needs no deploy — any browser. **Summit (same evening, ~19:05):**
root cause of the whole afternoon = TWO docker daemons fighting over
one socket (Docker Desktop + native dockerd both bound at different
moments); fixed by quitting + disabling Docker Desktop, restarting
docker.socket, re-clearing morning remnants on the native engine,
then one clean compose up + create-admin + login 200. Founder
logged in over the tailnet at
https://laptop-ltr9teil.tail042447.ts.net/host/ as host admin
(node renamed; `tailscale serve` on Windows — exactly the topology
recommended in finding #5). Journey B + C: passed by fire
(divergences = findings #1–#5, all fixed in-guide). Slice closes on
the A1–A12 landing report. **Finding #6 (2026-10-26 ~20:25, founder
redo journey):** rung 3's WSL note still taught the findings-#2/#3
workaround topology (Tailscale inside WSL + Windows app beside it,
"two nodes normal") instead of the summit path that actually worked
(Tailscale on Windows only, `tailscale serve` from PowerShell, WSL
tailscale removed). Note rewritten to lead with the clean path +
removal instructions; the two-node combo is now explicitly called
out as the trap (peer→WSL-node hangs), aligned with the "one
machine, one of each" topology rules. **Session pause (2026-10-26
20:36):** founder is mid-redo of the deploy journey on a
stranger-state machine (verified: no containers/volumes/networks/
images, `~/Tributary` gone incl. root-owned husk, port free, one
engine, one Tailscale). One prep command outstanding on his side:
`sudo apt install docker-compose-v2` (Ubuntu-native Docker ships
without the compose plugin — prerequisite line now names it). Redo
resumes from deploy/README.md "What you need"; landing A-clicks and
Parts B/C remain PASSed from the first pass — the redo is confidence
repetition, not a gate. **Finding #7 (2026-10-26 20:40, founder UX review):**
notes must precede the step they protect, not trail the rung — a
WSL stranger installed Tailscale inside WSL at rung-3 step 1 and met
the WSL note only after. Fix: a "WSL host? Stop" callout now sits
between the rung-3 intro and step 1, pointing at the Windows path;
the full note stays at the end (serve instructions read best after
the steps). Same ordering principle applied going forward: warnings
live at the decision point. **Slice 1 CLOSED (2026-10-26 19:31):**
founder reports the A click-through done (no FAILs raised); closing
self-check green — og:image 200, all landing hrefs point at real
files, `#quickstart` matches README `## Quickstart`,
`#sharing-ladder` matches the ladder heading. Journey verdict: A
PASS, B PASS (earned by five findings, all fixed), C done (tailnet
HTTPS host-admin login). M5.5 §14 tick stays reserved for the
Gemini slice.

**Tests/verification.** Screenshot rig 1/1 passed (4 scenes × 2 scales
× viewport/full = 16 real shots; all four @2x visually verified —
masked keys only, both story flags + /24 detail in frame, statuses
active/suspended/rotated visible); posture grep over
`landing/{index.html,style.css}` clean (sole matches: the
posture-rule comment itself and "feeds"); `/healthz` truth pinned in
the video script (HTTP 200, empty body — no invented JSON); Pages
workflow triggers on `landing/**` pushes to main + workflow_dispatch.
Founder journey (A1–A12, B1–B5, optional C) shipped — PASS pending
his run. Demo stack LEFT RUNNING (tributary dual-homed + demo-upstream,
unless-stopped) for the ~Nov-12 video record; re-stage commands live
in the video script's pre-stage section.

**Fast jump — slice 1 touched:** `landing/**` ·
`.github/workflows/pages.yml` · `deploy/README.md` ·
`docs/{demo-video-script.md,founder-journey-m55-slice1.md}` ·
`README.md` · `apps/dashboard/e2e/post_shots.spec.ts` ·
`scripts/post_shots.py` · `PLAN.md`


### 2026-10-25 — CHORE: launch-post assets — staged stack + scripted screenshots (M5.5 slice 1)

**Scope.** Director mini-kickoff (chore-grade, pre-M5.5, no milestone
tick, M5 untouched): stage a REAL running stack for the founder's public
launch post and script its screenshots via the J6 rig. Deliverables: (A)
a pull-path container left up with the fictional "Example Labs" org
seeded through the public API only (workflow step 12 — host bootstrap →
org → pool → members → keys → quota rules; metering history via the one
sanctioned exception, gateway owning the sole write path), staged stories
(a D32 two-/24 flag, a live classifier auto-block, a real 429 quota
denial, a rotation, D33 approval pairs), and a VISIT.md; (B) S1 review
queue + S2 audit log PNGs (dark-native, 1440px, 2× + 1× real shots, no
image editing) via a reusable in-repo rig.

**Staging facts.** The founder's stranger-run container was preserved
(stopped, renamed `tributary-stranger-run`, own volume) and a fresh
`tributary` container run from `ghcr.io/phat1911/tributary:latest`
(compose plugin still absent on this box — README's compose-free
`docker run` equivalent, decision 1). The host firewall blocks
container→host-gateway traffic here, so the mock (jev dialect + OpenAI
upstream roles, one port) runs as the `demo-upstream` container on a
second Docker network `demo-net` — which also gives the D32 story its
two GENUINE /24s: published-port traffic sources from 172.17.0.1, a
helper container on demo-net from 172.28.0.4 (decision 2). Classifier
configure + go-live each rode the D33 pending-action approvals
(avery requests, blake approves — two pairs in the audit log) before the
live off-purpose catch (decision 3). No header spoofing anywhere; every
detection row came from real served traffic.

**Files.** `scripts/mock_jev.py` (optional bind-host arg, default
unchanged — containers need 0.0.0.0) · new `scripts/seed_post_demo.py`
(stdlib; public-API provisioning + traffic stories; raw keys in memory
only, masked prefixes in all output; idempotent-fresh, demo throwaway
creds documented as demo-only) · new `scripts/post_shots.py` (J6-style
runner: npx playwright@1.56.1, shared target/pw-browsers cache) · new
`apps/dashboard/e2e/post_shots.spec.ts` (1440×900, deviceScaleFactor 2
and 1, clean context, dark palette — the product's only palette; waits
on real UI text, writes viewport + full-page variants) · `PLAN.md` (this
entry). Assets themselves live OUTSIDE the repo by design:
`~/my_project/Tributary-private/post-assets/` (8 PNGs + VISIT.md with
URL, credentials, page→story map, mock-vs-real precision lines).

**Tests/verification.** Seed script's own assertions all green:
provider-key probe ok, 429 observed pre-upstream + `security.quota_denied`
audited, exactly-one D32 flag, shadow counter ≥ 1, auto-block LIVE +
suspension 403 with recovery hint, rotation 201, both story flags open,
all five audit action kinds present. Screenshot spec 1/1 passed; both
shots visually verified (both flags in frame, /24 detail line visible,
masked keys only). Metering seed: 338 history rows inserted through a
stopped-container copy (WAL replayed first — 11 real rows recovered;
ownership restored to the container uid after docker cp).

**No new dependencies** (workflow step 10 n/a — the rig reuses the J6
playwright invocation and the existing browser cache; `python:3.13-alpine`
+ `node:20-alpine` are throwaway traffic-helper containers, not repo
deps).

**Notes.** Two staging lessons recorded for the M5.5 video re-stage: (1)
`docker cp` restores host ownership — chown back to the container uid or
every write 500s "readonly database"; (2) stopped containers may leave
today's data in the WAL — copy db+wal+shm together and let SQLite replay
before editing. The queue shows the live catch as two rows (score
evidence `classifier_flag` + `classifier_auto_block`); the shadow-leg
flag was dismissed to keep the queue at exactly the two stories. Stack
intentionally LEFT RUNNING (tributary + demo-upstream, unless-stopped).
Disclosure: the dashboard ships dark-only (by design, no toggle) — every
shot is dark because no light mode exists.


### 2026-10-22 — CHORE: member keys table — surface expiry + cap (pre-M5)

**Scope.** Director mini-kickoff (chore-grade, pre-M5): M4 already gave
pool keys `expires_at` and per-key token quotas, but the member "My pool
keys" table showed only Pool / Key / Status / Issued — members could not
see their own limits. Pure SURFACING: no new enforcement, schema, cron, or
second quota concept. Four design rulings received with the GO: cap
resolution = min-headroom across key/user/pool rules (the constraint that
actually blocks, anything else is a lie by omission); `used` = tokens in
the rule's current window, mirroring `quota.rs` enforcement (display never
diverges from what the gateway counts); the window is a visible muted
suffix (`12.4k / 100k · day`), not a tooltip (undiscoverable on mobile);
commit stages only explicitly-changed paths.

**Files.** `crates/control/src/db.rs` (`OwnPoolKey` + `expires_at` +
`cap: Option<OwnKeyCap>`; `OwnKeyCap {limit_tokens, period, used_tokens}`;
`tokens_used_since` — the twin of the gateway engine's `usage_tokens`
incl. the CAST-to-BIGINT Any-driver rule; pure `effective_cap` resolver;
`attach_effective_caps` orchestrator with a per-subject window-usage
cache; resolver unit tests); `crates/control/src/me.rs` (`GET /me/keys`
enriches rows); `crates/control/tests/self_service.rs`
(`member_key_list_surfaces_expiry_and_effective_cap`); `apps/dashboard/lib/api.ts`
(`OwnKeyCap`, extended `OwnPoolKey`); `apps/dashboard/lib/format.ts`
(`formatTokensCompact` — drops a trailing ".0" so the cell reads
`12.4k / 100k`; `formatTokens` itself untouched, other surfaces keep their
output); `apps/dashboard/app/me/page.tsx` (Expires + Cap columns;
`useWallClock` — the client clock as a `useSyncExternalStore` external
store with a cached snapshot: React-19 lint bans both `Date.now()` in
render (purity) and setState-in-effect, and the 0-valued server snapshot
keeps hydration mismatch-free); new `MEMBER-KEYS-FOUNDER-TEST.md`.

**Tests.** Journey (workflow step 12, public API only — org, pools,
members, keys with expiry bodies, quota rules all provisioned through the
API; only metering rows are seeded, the gateway owning their sole write
path): member A sees three keys — expiring-soon key with a key-level day
cap (`100k` limit, exactly `12.4k` used: a 50k row from before today's
window is excluded), an already-expired key (expires_at carried, status
still `active` — display-level only), a never-expiring key whose minute
request-RATE rule correctly surfaces NO cap ("Unlimited"); member B's
binding cap is their user-level day rule (50k/2k) beating the pool-level
month rule — the cross-subject min-headroom leg. Raw key material and
cross-member leakage asserted absent. Resolver unit tests: smallest
headroom wins across subjects, rate-only rules ignored, unknown period
canonicalizes to `day` like the engine, exhausted (used > limit) rules
still resolve. Gates: `cargo fmt --all -- --check` ✓, `clippy --workspace
--all-targets -D warnings` ✓, `cargo test --workspace` **132/132 ✓ dual-DB**
(SQLite + Postgres `trib_test_ci` :5433; self_service 4→5). Environment
notes, not code: one Windows `link.exe` LNK1000 flake over the UNC target
plus a stale running `tributary-gateway.exe serve` locking the binary —
the founder's server was left running, so the verified run used an
isolated Windows-local `CARGO_TARGET_DIR`; one transient Postgres connect
refusal under the parallel connect storm (harness anti-skip panic fired
as designed) — clean on rerun. Dashboard: `eslint` + `tsc --noEmit` +
`next build` ✓ (11 routes).

**No new dependencies** (workflow step 10 n/a — `formatTokensCompact` and
`useWallClock` are hand-rolled; no date library, per the M3 ruling).

**Notes.** `quota_rules.limit_cost` stays stored-but-unenforced — already
recorded as the D23 deferral (PLAN M1 notes + SPEC §5/D23 + the bind-site
comment in `api.rs`), so no new AUDIT line. The "expired" chip is
display-level by design: the DB has no expired lifecycle state and the
gateway enforces expiry at auth time. Revoked/rotated keys show "—" for
Cap (nothing enforceable); suspended keys keep theirs (restorable). No
milestone ticked — M5 owns the tracker. **Post-commit fix `e0fda1b`:** the
first founder run crashed (`RangeError: Invalid time value`) because a
PRE-chore `serve` binary was still running — it sends no `expires_at`, so
the field arrived as `undefined`, not `null`, and fell past the strict
guards; both cells now treat absent as Never/Unlimited (documented on the
interface — version skew must degrade, never crash). Same gates green
after the fix; restart the backend on the new commit to see live data.

### 2026-10-21 — D33 admin four-eyes (approvals) moved into M4

Director instruction (founder overruled the M6 deferral; SPEC already
amended by the director — the dirty SPEC.md is committed with this
change, same-commit amendment per workflow step 3).

**Scope.** Minimal two-step approval machinery for the exact D33
sensitive set: demote another admin; alter another admin's key (expiry,
revoke); org-wide policy (classifier purpose/auto-block configure,
remove, go-live); pool disable. Request by admin A → `pending_actions`
row (executes nothing) → a SECOND, DISTINCT admin (or the host) approves
→ executes; reject/withdraw discards. Audit rows for
requested/approved/executed/rejected/self-approve-denied. Guards:
requester can never self-approve; the last remaining admin cannot be
demoted (checked at request AND execution); single-admin orgs auto-pass
with a logged `"solo-admin auto-pass"` marker; containment exemption —
a key with an OPEN system alert (classifier, D32) may be acted on by
any single admin, the alert id linked in the audit row; system
suspensions (classifier auto-block) never create pending actions.

**Files.** `migrations/0011_pending_actions.sql` (new table);
`crates/control/src/approvals.rs` (kinds, gate, execution dispatcher,
sealed classifier payload); `crates/control/src/db.rs` (pending-action
queries, org_admin_count, demote, latest open flag);
`crates/control/src/api.rs` (gate helpers + hooks on expiry/revoke/
classifier PUT/DELETE/go-live/pool disable + list/approve/reject/demote
endpoints); `crates/gateway/tests/m4_journeys.rs` (j8); dashboard review
queue "Approvals" tab + `PendingAction` type.

**Tests.** j8 dual-DB journey through the public API: A expires B's key
→ 202 pending, key untouched → A self-approve 403 (audited) → B approves
→ executes → policy configure + go-live also defer and approve → live
classifier auto-block suspends B's key instantly with NO pending action
→ open alert lets A revoke solo → solo-admin org executes directly with
the marker. Approvals unit tests (kind round-trip, sealed payload
rebuild). Suite: 117/117 dual-DB, clippy clean both legs, dashboard
lint+tsc clean.

**No new dependencies** (workflow step 10 n/a).

Template:

```
### M<n> — <name> (date)
- changed: <paths>
- tests: <what ran, result>
- evidence: <SPEC "done when" check>
- limitations/notes: <known gaps>
```

### M3 — dashboards + audit log (2026-10-17)
- **changed:** `migrations/0006_audit_log_login_throttle.sql` (audit_log + login_throttle tables) · `crates/control/src/{lib.rs,error.rs,auth.rs,db.rs,api.rs}` + new `crates/control/src/{audit.rs,throttle.rs,usage.rs,me.rs}` · `crates/control/tests/harness/mod.rs` + new `crates/control/tests/{audit.rs,login_throttle.rs,cookie_session.rs,self_service.rs}` · `crates/gateway/src/lib.rs` (serve wiring for the new ControlState) · `apps/dashboard/` in full (app routes: login/host/org/org/pools/[id]/me/audit + root role-redirect; components: app-shell, data-state, sparkline, usage-table, secret-reveal, health-badge + shadcn/ui generated set; lib: api client, useApi hook, formatters; Tailwind v4 CSS-first theme in globals.css; eslint flat config + typecheck/lint scripts; README dev notes) · `.github/workflows/ci.yml` (dashboard job: lint + typecheck + build) · `pnpm-lock.yaml`
- **tests:** local (Windows-cargo WSL interop) — `cargo fmt --all --check` ✓, `clippy --workspace --all-targets -- -D warnings` ✓, `cargo test --workspace` **76/76 ✓ × two consecutive full runs** on the dual-DB matrix (SQLite in-memory A-002 branch + Postgres 16, throwaway Docker `tributary-pg-m3` on :5433, removed after): 13 common · 10 control lib · 7 control api · 3 audit · 2 cookie_session · 2 login_throttle · 4 self_service · 9 crypto + 1 doc · 12 gateway lib · 6 gateway api · 1 concurrency · 3 m2_isolation · 3 streaming. Dashboard: `eslint .` ✓ · `tsc --noEmit` ✓ · `next build` ✓ (8 routes, Turbopack). CI green on `main`: [run 39051018301](https://github.com/Phat1911/Tributary/actions/runs/39051018301) — Rust (fmt/clippy/test dual-DB via postgres:16 service) ✓, cargo-deny ✓, dashboard lint+typecheck+build ✓; first push ([39050675931](https://github.com/Phat1911/Tributary/actions/runs/39050675931)) failed because a pnpm dependency-install rewrite had silently dropped the new `lint`/`typecheck` script entries from `apps/dashboard/package.json` (local gates had invoked the binaries directly, so nothing caught it) — restored in 8efa119, verified both `pnpm -C apps/dashboard` forms exit 0. Dev-wiring smoke vs a throwaway mock control plane on :8081: `/login` renders, `POST /api/v1/auth/login` through the Next rewrite returns body + `Set-Cookie` intact, cookie-authenticated `/auth/me` proxied ✓.
- **evidence:** SPEC §14 M3 "done when" = *Org admin creates member + key + budget without CLI* — the dashboard org journey covers it end-to-end: login (cookie session) → /org "Add member" dialog (POST /members) → pool page "Issue key" (member picker → POST /pools/{id}/keys → raw key shown ONCE behind a reveal toggle, masked prefix thereafter) → "Set budget" dialog (pool- or member-scoped, minute/day/month windows → POST /quota-rules) → audit page shows each mutation with actor/target/timestamp. Backend guarantees verified by integration tests: `self_service.rs` (member sees ONLY own keys/usage; another member's key id → uniform 404), `audit.rs` (rows on every mutation incl. session.opened/closed; **no raw key material, hash, ciphertext, or password string anywhere in audit storage** — positive control is the masked prefix; org scoping of the viewer), `login_throttle.rs` (A-004 closure — see AUDIT), `cookie_session.rs` (Secure/HttpOnly/SameSite=Lax attributes, cookie + bearer parity, logout kills the session for both).
- **dependencies (AGENTS workflow step 10):** JS-side additions, all MIT/ISC/Apache-2.0: **tailwindcss v4 + @tailwindcss/postcss + postcss** (the M0-deferred activation the kickoff ordered; v4 is CSS-first — no tailwind.config, tokens as `@theme inline` in globals.css); **shadcn/ui generated components** with their runtime deps — `radix-ui` (unified primitives package behind dialog/select/tabs/alert-dialog/dropdown), `class-variance-authority` + `clsx` + `tailwind-merge` + `cn` (variant plumbing; `cn` is the official shadcn util package the current CLI templates import), `lucide-react` (icons), `sonner` + `next-themes` (mutation toasts; next-themes is sonner's theme integration), `tw-animate-css` (v4 replacement for tailwindcss-animate); **eslint + eslint-config-next** (devDeps — lint gate for CI); **typescript pinned to ^6 (6.0.3), down from ^7.0.2** — typescript-eslint does not support the TS 7 native compiler yet (eslint-config-next hard-fails on load); TS 6.x is the line supported by both Next 16 and the lint toolchain, and `tsc --noEmit` remains enforced. Deliberately NOT added: any chart library (the usage sparkline is inline SVG — a polyline is all a 14-day trend needs), framer-motion/3D (M5 visual polish scope), state-management libs (a 60-line useApi hook suffices). No Rust-side dependency changes (reused axum/sqlx/argon2/serde/tracing).
- **limitations/notes:** cookie `Path=/api/v1` means server components cannot see the session — all dashboard data fetching is client-side by design (the API stays the single authority); CSRF posture = SameSite=Lax + JSON-only mutation bodies (no endpoint accepts form-encoded writes — browsers auto-send forms cross-site, JSON does not), no token framework needed at this scale; the session cookie is always `Secure` — modern browsers honor it on `http://localhost`, and the login page's "silently bounces back to /login" symptom is documented in the dashboard README as the first thing to check in local dev; audit inserts are logged-not-propagated on failure (A-003 posture — the mutation already succeeded; a lost row is observable as an `error!` log) — outbox/retry if it ever fires; `ip` on audit rows and throttle buckets is first-XFF-hop-else-"local" (edge-proxy trust model — direct exposure would let a client rotate buckets by spoofing XFF; the per-email dimension still holds); dev proxy = Next rewrites to `TRIBUTARY_API_URL` (production packaging is M5, NO containerization this milestone); the host overview fetches provider keys per pool (no bulk endpoint — fine at M3 scale, noted in code); shared-PG parallel test interference eliminated by per-run unique identities in throttle fixtures + retry-tolerant shared-admin bootstrap (forensics: a globally fixed test email let sibling binaries' failed logins land in "our" throttle buckets — two full clean runs confirm the fix); member password reset/rotation UI deferred (members are recreated, not reset, at this scale); **post-demo follow-up (cab3d1d + d365860, 2026-10-18):** org_admins were only ever minted by direct test seeding — no API/UI path existed, which would have made the M3 done-when journey unreachable in real use; `POST /members` now takes an optional `role` (`org_admin` host-caller-only; `host_admin`/unknown rejected — host stays bootstrap-only) and the host overview gained an "Add admin" action per org (regression test: `host_provisions_org_admins_via_api`). **Live-verification sweep (user walkthrough, 2026-10-18, commits 471881c→fa8d543):** four more journey defects the gates missed — bare `GET /members` (missing required `?org_id=`) on the org page *and* the pool page's member pickers; no pool-creation UI; secret-reveal overflow; no org column on the host pools table — all fixed and verified live; a full client↔server contract cross-check (27 call sites) now shows no mismatches, and the final tree re-passed the complete dual-DB Rust gate + dashboard tsc/eslint/build (CI green through run 39108479555). Full defect log + process analysis: `M3-FINAL-REPORT.md`.

### 2026-10-22 — M4 round-2 adversarial scan (founder-ordered second wave)

Five fresh reviewers against 43e2591 (D33, classifier, gateway, rules
sweep, suite strength). Verdicts: 1 High + 6 Med + Lows; everything
exploitable FIXED, pinned red-first:

- **A-023 (High, fixed)** suspended-rotation chain: silent no-op +
  unblock resurrection (two live keys) + deferred orphan —
  `rotate_key_atomic` (guard `IN ('active','suspended')` + insert +
  allowlist copy in ONE tx) on both paths; refusal mints nothing.
- **A-024 (med, fixed)** dashboard dropped the approve reveal —
  SecretReveal now renders `issued` (was: UI-approved rotation = lockout).
- **A-025 (low, fixed)** approval.executed actor mixing.
- **A-026 (open, M5)** NO-BYPASS instances: quota-0 kill +
  delete_quota_rule + revoke_provider_key; ungated allowlist edits on
  another admin's key; admin-target key issuance. Folded into the
  director-ruled A-022b workstream; (c) needs a director call.
- Suite strengthened per the round-2 test review: D32 three-surface proof
  (flag + audit row + email-outbox row, one flag id), D33 containment
  audit alert-link (j11), rotation reveal negatives (audit + executed
  list + stored payload), v6 /64 journey legs, mutual-demote now 3 fresh
  orgs, five weak asserts hardened, adv_live determinism barrier.
  12 journeys (was 11), 127 workspace tests, clippy -D warnings 0, fmt
  clean, dashboard lint+tsc clean.

  Late round-2 additions (classifier reviewer reported after the first
  push): **A-027 (med, fixed)** live-preservation narrowed — vendor
  (base_url/dialect/model) or purpose swaps re-enter shadow on both PUT
  and deferred execute; sampling/key tweaks keep live (regression legs
  in adv_live). **A-028 (med, M5)** go-live approve-ordering zero-shadow
  arm — folded into the A-022b staleness family. **A-029 (med, fixed)**
  PG bench-lock deadlock in concurrent demote executions — ORDER BY id
  on the FOR UPDATE select (CI caught a (0,2) flake; local repro showed
  the loser 500ing); mutual-demote journey now 3 rounds with approve
  response diagnostics.

### 2026-10-22 — M4 final-scan adversarial close-out (5 reviewer subagents)

**Scope.** Founder-directed milestone-close scan: independent adversarial reviewers attacked the four M4 feature areas (four-eyes/D33, classifier/D28, gateway leak prevention, suite strength); every exploitable finding fixed in this commit, every design-level finding recorded in AUDIT.md A-019..A-022 for the director.

- **changed:** `crates/control/src/approvals.rs` (new `AdminKeyRotate` kind + `Executed{summary,reveal}` — reveal-once material reaches only the approver's response; live-preservation rule on the deferred configure path; `audit_view` strips ciphertext/dek_wrapped from audit copies) · `crates/control/src/db.rs` (`claim_pending_action`/`finalize_pending_action` status-transition CAS; `demote_org_admin_guarded` — guard inside the UPDATE + `FOR UPDATE` bench lock on PG; `copy_ip_allowlist`; Serialize derive dropped from PendingActionRow) · `crates/control/src/api.rs` (rotate now D33-gated with dual 202/201 response + allowlist carry; approve claims BEFORE executing; `ApproveActionResponse{action,issued?}`; marker honesty — routine actions carry no `dual-approved` lie; `approval.executed` actor-consistent + carries mode; strict status enum; probe-error redaction; classifier PUT live-preservation + audit `before`; host-demote-last-admin → 409; char-safe `display_prefix`) · `crates/common/src/db.rs` (`pub fn postgres()` for lock-dialect selection, dual-DB rule documented) · `crates/gateway/src/chat.rs` (garbage-CIDR rows fail CLOSED) · `crates/gateway/src/classifier.rs` (char-boundary-safe prompt cap + honest module doc) · `crates/control/src/classifier.rs` (non-finite scores fail OPEN) · `apps/dashboard/app/org/review/page.tsx` (alignment was rendered 10× high: `/1000` vs the ×10_000 bp writer) · test harnesses (`common/mod.rs` + control `harness/mod.rs` now PANIC when TRIBUTARY_TEST_DATABASE_URL is set but PG is unreachable — a silent skip was proven to pass SQLite-only; journey `bootstrap_admin_and_login` retries the shared-PG login race)
- **test repairs (rule 6 upheld — asserts only strengthened):** j4/j5 audit waits key-scoped (stale rows on the shared PG DB satisfied the old ones — proven false-pass), j5 wait → direct assert (the gateway awaits the audit write), j6 dedupe counts strict (`expect`, not `unwrap_or(0)` tautology) + exactly-1 pinned, j4 garbage-CIDR proves nothing stored (len==1), j8 summary date-prefix de-bombed ("2026-" → "2"), j8 approve asserts moved under the new `{action, issued?}` wrapper
- **added adversarial journeys (all RED against pre-fix code):** `adv_d33_identity_scope_matrix_and_restart` (member 403 / second-session 403 / cross-org + spoofed-org 404 / restart persistence with zero side effects / host-as-second-eye / routine-never-defers), `adv_d33_race_reject_vs_execute_world_matches_row` (5× `tokio::join!` approve∥reject; world⇔row consistency), `adv_d33_mutual_demote_never_leaves_org_adminless` (concurrent cross-approved mutual demotes → exactly one executes, ≥1 admin remains), `adv_rotation_gated_and_carries_allowlist` (routine + deferred rotation both keep the CIDR pin; approver receives the reveal-once raw; old credential dead), `adv_live_auto_block_spares_on_purpose` (threshold-inversion gap — LIVE mode must never flag above-threshold traffic; mock hit-counter is the determinism barrier); unit tests: multibyte prompt-cap straddle (3-byte char at byte 8192 — the exact panic), non-finite score parsing
- **evidence:** `cargo test --workspace` dual-DB (SQLite + Postgres 5433/trib_test_ci) **all green**; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean; dashboard `eslint .` + `tsc --noEmit` clean. Reviewer-held lines (verified unbroken): self-approve denial incl. second sessions, org scoping of pending rows, payload immutability + read-time heal, containment sunset on resolve, deferred-request zero-side-effects, restart safety, dual-DB portability of every M4 query, rule-2 masking across logs/flags/audit/email, member disclosure
- **AUDIT:** A-010..A-018 fixed this commit; A-019 (XFF first-hop trust), A-020 (quota TOCTOU burst), A-021 (D32 dedupe race), A-022 (delete_org/org_settings gating + configure last-writer-wins + kind check) open for the director — none blocks M4 close, all recommended for the M5 kickoff prompt
- **no new dependencies**

### M4 — leak prevention free tier: canaries, IP allowlists, review queue, email alerts, BYOK classifier (2026-10-18)

- **changed:** `migrations/0007_leak_prevention.sql` (ip_allowlists, pool_key_ips, canary_models, review_flags, email_alerts outbox, classifier_config, classifier_scores; pool_keys.status value-space extends to suspended/rotated — documented, no schema change) · `crates/common/` (new `src/net.rs`: client_ip + Cidr parse/contains + fail-closed allowlist verdict; Cargo +`http` dep) · `crates/control/` (new `src/{classifier.rs,notify.rs,security.rs}`; db.rs +M4 row types/queries incl. the shape-uniform shared listing vs. the admin management view; api.rs +key governance — rotate/expiry/allowlists CRUD (GET added for the editor), canary-keys planting with backdated interleave, canary-models CRUD, review-flags list/resolve (unblock/revoke/dismiss), classifier PUT with save-time probe + auto-block acknowledge + shadow + go-live, DELETE; me.rs +`/me/disclosure`; Cargo +`lettre`) · `crates/gateway/` (new `src/{classifier.rs,geo.rs}`; keys.rs honey-key inline detection → generic 401 + suspended 403; chat.rs canary-model local deny (same 400 as allowlist), IP-allowlist deny + audit, per-key IP history + dormant travel check, async sampled scoring spawn; models_api.rs merges member-visible canary aliases; db.rs +canary/allowlist/ip-history queries; config.rs/state.rs smtp+geoip plumbing; Cargo +optional `geoip` feature → `maxminddb`) · `crates/gateway/tests/` (+`m4_journeys.rs`, common/mod.rs +captured_count, Config literal fields) · `apps/dashboard/` (pool page keys tab → management view + rotate/expiry/allowlist editor + canaries tab; new `app/org/review/page.tsx` + `app/org/classifier/page.tsx`; me page disclosure banner; api.ts +put +M4 types; app-shell nav) · `.github/workflows/ci.yml` (geoip clippy leg) + new `classifier-smoke.yml` (dormant nightly) · `scripts/{manual_journey_m4.py,mock_jev.py}`
- **tests:** local (Windows-cargo WSL interop) — `cargo fmt --all -- --check` ✓, `clippy --workspace --all-targets -- -D warnings` ✓, `clippy -p tributary-gateway --features geoip -- -D warnings` ✓, `cargo test --workspace` **105/105 ✓ on the true dual-DB matrix** (SQLite always + Postgres 16 via Docker `tributary-pg-test` :5433 — WSLENV-forwarded `TRIBUTARY_TEST_DATABASE_URL`; the Postgres half verified by table counts in the test db, not just exit codes): 17 common · 19 control lib · 8 control api · 3 audit · 2 cookie · 2 throttle · 4 self_service · 9 crypto + 1 doc · 16 gateway lib · 6 gateway api · 1 concurrency · 3 m2_isolation · 3 streaming · 1 models · **5 m4_journeys** (each loops both DBs) · 6 classifier contract fixtures. Dashboard: `eslint .` ✓ · `tsc --noEmit` ✓ · `next build` ✓ (10 routes). **Founder journey script vs the real running binary on real Postgres** (Windows exe + WSL Docker PG): all four journeys + both director directives pass — see evidence.
- **evidence:** SPEC §14 M4 done-when, journey-first per workflow step 12: `crates/gateway/tests/m4_journeys.rs` provisions EVERYTHING through the public API (host-admin bootstrap → org → pool → provider key → member → issued key → plant canaries/configure classifier) — **J1** honey key rides the shared listing, use → generic 401 **byte-identical** to an unknown key's, zero upstream calls, zero usage rows, review flag + audit row + outbox row; **J2** canary alias in the member's /v1/models, request → the exact allowlist 400 locally, flag + audit + alert; **J3** failed probe rejects the save, alert-mode flag carries score_bp and **no prompt text anywhere**, disclosure active in Alert mode, unacknowledged auto-block → 400 (the warning), acknowledged → shadow ("would have blocked ≥ 1"), go-live → key suspended → 403 with recovery hint → one-click unblock → serves again; **J4** CIDR deny outside + audit row, admit in-range, fail-closed on direct/local, delete → dormant. Director directive 1 has its own test: `shared_listing_shape_identity_across_kinds` — identical field sets (exactly the uniform five), identical value types per field, no kind/user_id key anywhere, created_at ordering with canaries interleaved (not clustered). The same journeys then ran against a **real server process** (`scripts/manual_journey_m4.py --ci`, real binary, real Postgres, in-process mocks) — which is what caught the Postgres-only bug below.
- **dependencies (AGENTS workflow step 10):** new this milestone — `lettre` 0.11 (default-features off; builder+smtp-transport+tokio1+tokio1-rustls-tls: the SPEC §9 email alert, MIT/Apache-2.0, rustls keeps the no-native-TLS posture); `maxminddb` 0.32 (**optional** behind the `geoip` feature, ISC/Apache-2.0, pure Rust — the impossible-travel country lookup; feature OFF by default so the default build carries no GeoIP dependency, per the dormancy ruling); `http` 1 in common (HeaderMap without a framework dep for client_ip). Deliberately NOT added: no templating library (plain-text builders), no async-trait (hand-rolled Pin<Box<dyn Future>> keeps ClassifierProvider object-safe), no GeoIP in the default build, no scheduled prober (D17: detection free, automation paid).
- **limitations/notes:** classifier sampling is check-then-insert (approximate under concurrency — acceptable for a sampling knob); scores/thresholds are INTEGER basis points (no REAL columns — cross-DB exactness); email delivery is fire-and-forget with the outbox row as the durable record (SMTP unconfigured ⇒ logged fallback — the founder's manual journey with real TRIBUTARY_SMTP_* is the live proof of that leg); allowlist denial is audit-only (no queue item — misconfigured clients are loud, not malicious); impossible travel fires on any country DIFFERS (no velocity math — needs the geoip feature + a GeoLite2 db, otherwise pure history write) — and explicitly: the `geoip` feature is OFF in every build shipped or tested this milestone, so `MaxMindDb` has never executed anywhere (CI checks it compiles clean via the dedicated clippy leg, and the pure `travel_verdict` fn is unit-tested, but the `Reader::open_readfile`/lookup path against a real `.mmdb` file is UNTESTED — first real compile+run waits on the GeoLite2 acquisition decision (licensing + download), M5 candidate); the classifier prompt extract is capped at 8 KB and never persisted (D28); **A-006 (fixed this milestone):** `upsert_classifier_config` bound `auto_block_live` as a Rust bool — sqlx maps that to BOOLEAN, Postgres rejects it for the BIGINT column, SQLite's loose typing masked it locally; caught by the real-server journey script, fixed with `as i64` (the dual-DB INTEGER-0/1 rule now stated at the bind site); process lesson recorded here: the earlier "dual-DB green" suite runs had silently skipped the Postgres half (WSLENV must list `TRIBUTARY_TEST_DATABASE_URL` for the Windows-cargo interop — a grep for `test result:` alone cannot see a skipped half; the definitive check is tables-in-the-test-db, which is now part of the local gate habit); a `.gitignore` entry for the (tracked) `M3-FINAL-REPORT.md` appeared in the worktree mid-session from outside this session's edits — committed as-is (inert while the file stays tracked) and flagged to the director; **post-close follow-up (2026-10-19, founder review):** canary-alias enumeration by insiders (A-008) — shipped the enumeration-pattern detection enrichment (`canary_hits_for_key` + `enumeration_pattern`/`prior_hits_24h` flag detail + review-queue ⚠ chip + J2 assertions); true prevention (per-member unique alias sets) is a director decision queued in A-008 for M5; honey-key listing indistinguishability is statistical at the created_at level (uniform backdate into [pool.created_at, now]) — the shape/ordering invariants are tested deterministically; auto-block scoring rides the request's spawn (single binary, SPEC §13) — a queue + worker split is M6+ scale work if ever needed; **also explicitly NOT built (founder asked 2026-10-19):** no check for one pool key used from two different IPs — the `pool_key_ips` history is collected but write-only (no IP-differs alert, no concurrent-use detection, no admin surface listing a key's IPs); concurrent multi-IP use is the classic credential-sharing signal and needs no geoip, so it is queued as an M5 candidate alongside the geoip decision.

- **REMOVAL — D31 canary pull-out (2026-10-19, director instruction, founder-tested decision):** honey keys + canary model aliases removed entirely, same-commit SPEC amendment (§9 free/paid restated, §14 M4/M6 reworded, D17 marked SUPERSEDED, D31 appended; AUDIT A-009 low/accepted, A-008 superseded). Code: `crates/control/src/{db,api,lib,security}.rs` (plant/list/add/remove handlers + queries + `HoneyKeyUsed`/`CanaryModel` kinds + rotate/allowlist canary guards gone; `ManagedPoolKey.kind` dropped), `crates/gateway/src/{keys,chat,models_api,db,lib}.rs` (honey 401 branch, canary-model deny + enumeration enrichment, models-merge, lookups gone), `migrations/0008_remove_canaries.sql` (drops `canary_models`, deletes canary pool_keys rows; `kind` column retained — always `live`), `m4_journeys.rs` J1/J2/shape-identity deleted. **Added for the amended done-when:** quota denial now writes `security.quota_denied` audit row (chat.rs; no flag/email — a cap firing is house rules, not a leak signal), proven by new `j5_quota_cap_audited` (provision via public API → day cap 10 → first request 200/burns 18 → second 429 before upstream → audit row present; loops both DBs). Dashboard: Canaries tab + Plant/AddCanary dialogs + amber honey rows + kind badges + review-queue ⚠ chip removed (api.ts types pruned; no standalone export action existed — the plant dialog's reveal-once list was the only raw-key surface and is gone). Kept intact per instruction: J3 classifier, J4 IP allowlist, per-key IP history + dormant geo check, quotas-as-cap, email alert infra (now serves classifier flags), review queue, disclosure banner, suspension/unblock. Scripts: `manual_journey_m4.py` J1/J2 stripped, J5 quota journey added; `M4-FOUNDER-TEST.md` T1/T2 removed, quota suite added. Commit: 210d1e8. Tests re-run: fmt ✓, clippy ×2 ✓, `cargo test --workspace` dual-DB (SQLite + Postgres via Docker :5433, tables-verified) ✓ — count now 102 (m4_journeys 5→3: J3, J4, J5), dashboard eslint+tsc+build ✓, journey script vs real binary+Postgres ✓ (after two script-side finds: the 5 s rules-cache TTL needs a wait between setting a quota rule and testing it — the rust suite's first chat was a natural cache miss, masking this; and a create-admin failure no longer leaks the spawned server, which held the journey DB open and broke the next run's DROP DATABASE).

- **ADDITION — D32 distinct-IP concurrency detector (2026-10-20, director instruction, founder-directed):** same pool key used from ≥2 distinct shared networks (v4 /24, v6 /64) inside a sliding window → flag + audit + email, ALERT-ONLY (never blocks). Fills the M5-candidate gap recorded in the M4 log ("no check for one pool key used from two different IPs"). Implementation rides the existing per-key IP history (`pool_key_ips` upsert keeps one row per (key, ip) with last_seen — the active-source query is O(active sources), not O(history)) and the D9 in-proc cache pattern (`OrgSettingsCache`, 5 s TTL, QuotaEngine-style Arc). New: `migrations/0009_org_settings.sql` (org-tunable `concurrency_window_secs`, default 300, clamped 60–86400 at the API), `GET/PUT /api/v1/orgs/{id}/settings` (audit-logged), `shared_network_of`/`parse_shared_network` in common/net.rs (+ unit tests), `DetectionKind::DistinctIpConcurrency`, `active_ips_for_key` (gateway db), detection block in chat.rs after the IP upsert with **one-alert-per-key-per-window dedupe** (count same-kind flags inside the window — survives restarts, unlike in-proc state). Journey `j6_distinct_ip_concurrency`: same-/24 pair stays quiet, second /24 fires the flag with `shared_networks ≥ 2` while the request still 200s (alert-only proven at the protocol level), third network adds NO second flag (dedupe proven). Dashboard: review-queue label + networks detail line. Director's instruction named the journey "J5"; committed as **j6** — j5 is the quota-cap journey added by the D31 removal commit (numbering moved on).

- **ADDITION — Jev dialect (real contract, director research 2026-10-21):** the version pin absorbed reality exactly as designed — the org's classifier config gains a `dialect` (`migrations/0010`, default `'custom'` so pre-dialect rows keep working): `"jev"` = TypeSafe AI's hosted contract (`POST {base}/v1/systemone`, noul-typed alignment question, score at `answers.alignment.noul`, `state.message` capped 4096 chars at char boundaries; empty `base_url` defaults to `https://api.typesafe.ai`, default model `jev-latest`, pinning allowed); `"custom"` = the generic `/v1/align` shape this crate shipped originally. Adapter-only change: scoring pipeline, flags, shadow/go-live untouched. Error surface per the vendor docs: **fail-open always**; 429/529 trip a 60 s per-org cooldown (`ClassifierBackoff` in-process — a wrong decision costs a few skipped scores, so no durability), 401/422 log loudly as config problems. Save-time probe = ONE minimal real noul call with the org's REAL purpose (a malformed statement fails at save, not on first request). Pricing surfaced in the dashboard form: $0.042/M input tokens, output free, ≈ $0.03/day at the default 30/hour sampling. Tests: systemone fixtures + contract tests (`tests/fixtures/jev/systemone*.json`), unit tests for body shape/char-boundary cap/dialect parsing, journey **j7_classifier_jev_dialect** (wire-level assertion of the documented request shape + flag + prompt-never-stored), j3 pinned to `custom`. Manual mock (`scripts/mock_jev.py`) serves `/v1/systemone` so the founder can exercise the real dialect locally.

- **CHORE — supply-chain bump (workflow step 10):** `rustls-webpki` 0.103.15 → **0.103.16** and `webpki-roots` 1.0.9 → **1.0.10** (semver-compatible lock bumps, no manifest change). Fresh RUSTSEC advisories — 2026-0382 (distrusted emSign intermediates in webpki cert verification) and 2026-0381 (bundled root store) — flagged the pinned versions in cargo-deny; both fixes are patch releases from the same maintainers. Transitive-only: pulled in via reqwest/lettre/rustls; no API usage in our code, so the upgrade is a lock refresh, justified purely as advisory hygiene.

### M2 — orgs, pools, provider keys (encrypted), quota/rate engine (2026-10-17)
- **changed:** `migrations/{0003_orgs_users_sessions.sql,0004_pools_provider_keys_quota.sql,0005_usage_events_pool.sql}` · `crates/common/` (src/db.rs moved from gateway + keys.rs + time.rs, Cargo.toml) · `crates/crypto/` (full crate: MasterKey/seal/open) · `crates/control/` (full crate: Cargo.toml, src/{lib.rs,error.rs,auth.rs,db.rs,api.rs}, tests/{harness/mod.rs,api.rs}) · `crates/gateway/` (src/{main.rs,lib.rs,config.rs,state.rs,error.rs,keys.rs,db.rs,quota.rs,routing.rs,chat.rs,models_api.rs}, tests/{common/mod.rs,api.rs,streaming.rs,concurrency.rs,m2_isolation.rs}) · `AGENTS.md` (teaching-comments convention) · `SPEC.md` (D30 adoption sweep) · `Cargo.lock`
- **tests:** local (Windows-cargo WSL interop) — `cargo fmt --all` ✓, `clippy --workspace --all-targets -- -D warnings` ✓, `cargo test --workspace` **59/59 ✓** on the full dual-DB matrix: SQLite (in-memory, A-002 branch) **and** Postgres 16 (throwaway Docker container `tributary-pg-m2` on :5433, `TRIBUTARY_TEST_DATABASE_URL`, removed after) — 4 crypto lib · 13 common lib · 4 control lib · 7 control api · 12 gateway lib · 6 gateway api · 1 concurrency · 3 m2_isolation · 3 streaming. CI green on `main`: [run 38996743570](https://github.com/Phat1911/Tributary/actions/runs/38996743570) — Rust (fmt/clippy/test, dual-DB via postgres:16 service) ✓, cargo-deny ✓, dashboard ✓; no CI changes were needed.
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

**M7 — LedgerTrait + double-entry ledger (2026-10-28):**

- Migration: `migrations/0015_ledger.sql`
- Guards: `crates/common/src/db.rs` (`run_ledger_guards`, hooked into `run_migrations`)
- Ledger crate: `crates/ledger/Cargo.toml` · `crates/ledger/src/{lib,money,chart,posting,ledger,error}.rs` · `crates/ledger/tests/{harness/mod,append_only,journey,properties,alarms}.rs`
- Docs/records: `deploy/README.md` (ledger append-only boundary) · `SPEC.md` (D38 + §14 tick) · `AUDIT.md` (A-030/A-031/A-032) · `PLAN.md` (this entry)
- Lockfile: `Cargo.lock` (ledger dependency edges only — no new crates)

**CHORE — launch-post assets: staged stack + scripted screenshots (2026-10-25):**

- Scripts: `scripts/mock_jev.py` (bind arg) · new `scripts/seed_post_demo.py` · new `scripts/post_shots.py`
- Shots: new `apps/dashboard/e2e/post_shots.spec.ts`
- Docs: `PLAN.md` (this entry); assets outside the repo at `~/my_project/Tributary-private/post-assets/` (PNGs + VISIT.md)

**CHORE — member keys expiry + cap surfacing (2026-10-22):**

- Control: `crates/control/src/db.rs` (OwnPoolKey/OwnKeyCap + resolver) · `crates/control/src/me.rs` · `crates/control/tests/self_service.rs`
- Dashboard: `apps/dashboard/lib/api.ts` · `apps/dashboard/lib/format.ts` · `apps/dashboard/app/me/page.tsx`
- Docs: `MEMBER-KEYS-FOUNDER-TEST.md` · `PLAN.md` (this entry)

**M4 (2026-10-18):**

- Migration: `migrations/0007_leak_prevention.sql`
- Common: `crates/common/src/{lib.rs,net.rs}` · `crates/common/Cargo.toml`
- Control (detection pipeline + governance API): new `crates/control/src/{classifier.rs,notify.rs,security.rs}` · `crates/control/src/{lib.rs,db.rs,api.rs,me.rs,auth.rs,audit.rs}` · `crates/control/Cargo.toml` · new `crates/control/tests/classifier_contract.rs` + `crates/control/tests/fixtures/jev/*.json`
- Gateway (data-plane gates): new `crates/gateway/src/{classifier.rs,geo.rs}` · `crates/gateway/src/{lib.rs,main.rs—unchanged,config.rs,state.rs,keys.rs,db.rs,chat.rs,models_api.rs,routing.rs}` · `crates/gateway/Cargo.toml` · new `crates/gateway/tests/m4_journeys.rs` · `crates/gateway/tests/common/mod.rs`
- Dashboard: `apps/dashboard/lib/api.ts` · `apps/dashboard/app/org/pools/[id]/page.tsx` · new `apps/dashboard/app/{org/review,org/classifier}/page.tsx` · `apps/dashboard/app/me/page.tsx` · `apps/dashboard/components/app-shell.tsx`
- CI: `.github/workflows/ci.yml` · new `.github/workflows/classifier-smoke.yml`
- Scripts: new `scripts/{manual_journey_m4.py,mock_jev.py}`

**M3 (2026-10-17):**

- Migrations: `migrations/0006_audit_log_login_throttle.sql`
- Control (hardening + new surfaces): `crates/control/src/{lib.rs,error.rs,auth.rs,db.rs,api.rs}` · new `crates/control/src/{audit.rs,throttle.rs,usage.rs,me.rs}` · `crates/control/tests/harness/mod.rs` + new `crates/control/tests/{audit.rs,login_throttle.rs,cookie_session.rs,self_service.rs}`
- Gateway wiring: `crates/gateway/src/lib.rs`
- Dashboard (M0 shell → real app): `apps/dashboard/app/{layout.tsx,page.tsx,globals.css}` + routes `apps/dashboard/app/{login,host,org,org/pools/[id],me,audit}/page.tsx` · `apps/dashboard/components/{app-shell.tsx,data-state.tsx,sparkline.tsx,usage-table.tsx,secret-reveal.tsx,health-badge.tsx}` + `apps/dashboard/components/ui/*` (shadcn generated) · `apps/dashboard/lib/{api.ts,use-api.ts,format.ts}` · `apps/dashboard/{package.json,tsconfig.json,next.config.ts,next-env.d.ts,postcss.config.mjs,eslint.config.mjs,components.json,README.md}`
- CI: `.github/workflows/ci.yml` · Lockfile: `pnpm-lock.yaml`

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
