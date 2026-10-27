# PLAN.md — Milestone status & log

> Working document. Status lives here; the permanent tracker with checkboxes lives in SPEC.md §14.

## Current state

- **Date:** 2026-10-27 · **Spec:** v0.3 (decisions D1–D36)
- **M5.5: DONE (closed 2026-10-27, slice 2 = Gemini)** — slice 1 (2026-10-26): landing live at https://phat1911.github.io/Tributary/, Tailscale rung stranger-proofed (findings #1–#7), video script, journeys A/B/C PASS. Slice 2 (2026-10-27): Gemini as a first-class pool provider — proven provider-agnostic by construction (no gateway-code change; base_url + Bearer already spoke the compat endpoint). J-A/B/C public-API journeys dual-DB green (`m55_gemini.rs`); J-D official-SDK contract legs vs mock Gemini-compat upstream green (same CI script); dashboard provider select + column; README/deploy/landing honest paragraphs + model-ID verify curl + rate-limit honesty; J-E founder real-key script shipped (`docs/founder-journey-m55-slice2.md`). Demo stack RE-STAGED collision-proof (`tributary-demo` @8112, dual-homed on demo-net, stories green, shadow flag dismissed, usage parity re-seeded: 281 rows / 15 days / 460K tokens). **Residue until real: demo video records ~2026-11-12, README link lands same day — Nov-7 director checkpoint chases it.** Next: M6 (pilot onboarding; adoption focus, V1 entirely free per D35).
- **Code:** M5 complete and director-verified (2026-10-25): full hardening block red-first (A-019/020/021/022bc/026/028), packaging + GHCR v0.1.0 live, SDK contract tests in CI, public scrub + full-history rewrite clean, founder stranger run PASSED. Close-out 53c0430. Known leftovers: stopwatch number (optional, M5.5), D32 sequential-sharing blind spot (documented), concurrency_flags_for_key cleanup decision (open, pre-M5 leftover).
- **Post-close events (2026-10-25/26):** LinkedIn humble-launch post shipped (four ask doors per D34); launch-post assets chore done (79bbb80 — seed_post_demo.py, post_shots rig, stack staging); stranger-finding #4 fixed live: quickstart master-key step promoted to its own line (969cfdf); GDG hackathon entry recorded (D36); Product Hunt launch planned for early December post-M5.5 (feedback-not-upvotes; vault holds the doctrine).

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
| M6 | pending | V1 — pilots |
| M7–M11 | pending | V2 — Root Pool |
| M12–M14 | pending | V3 — Operator network |

## Milestone log (append per completed milestone; keep the most recent 10)

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
and DSH → pool → OpenAI.

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
