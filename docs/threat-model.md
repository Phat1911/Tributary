# Threat model — and its honest boundaries

> This document states what Tributary defends against, how that defense is
> verified, and — just as deliberately — what has **not** been independently
> reviewed yet. A security doc that only lists strengths is marketing.
> Findings live in [`AUDIT.md`](../AUDIT.md); process lives in
> [`AGENTS.md`](../AGENTS.md).

## Trust boundaries and assets

Tributary is self-hosted governance for one organization. The model in one
table:

| Who/what | Trust level | Why |
|---|---|---|
| The org running the software (host, `.env`, DB file/instance) | **trusted** | Self-hosting premise: whoever owns the machine owns the data. The guards below bind the *application's* connection, not the file owner. |
| Org admins | trusted, **but recorded** | Every admin action lands in the append-only audit log; the trail is the check on admins. |
| Members | **untrusted** | Per-member keys, quotas, and rate limits are enforced at the gateway BEFORE any upstream call (rule 4). Members are who leak-detection watches. |
| Upstream AI providers | semi-trusted | They see the traffic you send with the key you pooled — no more than running their SDK directly. Keys are envelope-encrypted at rest and only decrypted per-use in memory. |
| The network between members and the gateway | **untrusted** | TLS is the operator's boundary (see the deploy guide's rungs); cookie sessions are `Secure`; API keys never appear in URLs. |

**Assets:** provider keys (sealed at rest under the master key), member
usage data, the audit trail, and quota fairness (reserve→settle→release, so
no member spends unaccounted).

**Database-level walls:** `ledger_entries` and `audit_log` are append-only
at the engine level — SQLite `RAISE(ABORT)` triggers re-armed on every
start; Postgres `tributary_app` role split holding only `SELECT`+`INSERT`.
Burglar tests attack both tables as the app connection on every CI run,
both engines. The honest boundary: these bind the application's connection;
the file/DB owner can always drop the walls (see the deploy guide's
"Append-only boundary" section for the full statement, including the
application-role flip that M10 wires up).

## What is audited, and how

- **Adversarial audit sessions.** Security-relevant milestones get a
  dedicated attack session (a fresh reviewer context, a named persona per
  surface — e.g. a smart-contract security auditor for the Solana program,
  a payment-fraud specialist for the ledger). Attacks go through the public
  API surface first, then threat-model-driven internals, plus a recursive
  pass asking whether the test suite actually encodes the rules it claims
  to.
- **PoC or no finding.** Every finding ships as a **reproducible
  proof-of-concept** in the public [`AUDIT.md`](../AUDIT.md) with a
  severity. No PoC, no finding — and attack categories that came back
  clean are listed as *attempted-clean*, so silence is documented rather
  than assumed.
- **The loop closes in public.** Findings route to a fix session; they
  close only when fixed or formally accepted with rationale in AUDIT.md.
  Nothing is silently deleted.
- **Continuous, in CI:** every push runs the full suite on BOTH database
  engines (SQLite + Postgres), the burglar alarms above, official
  OpenAI-SDK contract tests against the compatible surface, property tests
  for ledger invariants (trial balance, solvency), `cargo-deny` for the
  supply chain, and lint/typecheck/build for the dashboard.

**Audited so far, with receipts:** M4's two adversarial waves (12 journeys,
findings A-019…A-029 ruled in and fixed in M5), the M5 hardening batch
(trusted-proxy XFF spoofing A-019, quota atomicity A-020, dedupe A-021,
sensitive-action gates A-026), the M8 Solana program's Session C
(deployment-authority front-run A-041 and event-mirror truth A-042 fixed;
A-043 accepted-until-M10 with a standing regression pin), and the advisory
triage trail (e.g. A-045).

## What is NOT independently reviewed yet

Stated plainly, because strangers should know where the walls are thin:

- **Key-vault crypto** (`crates/crypto`): envelope encryption with
  XChaCha20-Poly1305 from RustCrypto and per-use in-memory decryption.
  The primitives are standard; the *integration* has had no external
  cryptographic review. **Flagged as seeking external review.**
- **Gateway auth path**: the full chain of session issuance/validation and
  member-key verification (login throttling, cookie flags, key hashing) is
  journey-tested and CI-pinned but has not had an independent end-to-end
  audit. **Flagged as seeking external review.**
- **Depositor mode** (M9, root pools — experimental): the adversarial
  audit session is scheduled for this week; until it runs and its findings
  close, treat `kind:"root"` as exactly what the README says:
  experimental.

If you review either of the flagged areas and find something: disclosure
path below, finder credit in AUDIT.md, PoC-or-no-finding applies to us too.

## Bus factor

One maintainer builds this today. That is a real risk and it is mitigated
the only way a small project honestly can: everything a successor needs is
in the repo — SPEC.md (decisions D1–D43 with rationale), PLAN.md (what
happened and why), AUDIT.md (what is broken or accepted, with PoCs),
teaching comments through the code, dual-engine CI that enforces the
invariants, and a license (FSL-1.1-ALv2) that converts every version to
Apache-2.0 two years after release — so even total maintainer loss leaves
a self-hostable, forkable product. No critical operation depends on any
hosted service of ours; there is none.

## Support and disclosure

- **Support path: GitHub issues only.** No paid support tier, by design
  (D35): V1 is entirely free, and support-as-a-product would distort the
  incentive this repo is built on. Pilot organizations: the issue
  templates and the runbooks in `docs/` are the intended first stop.
- **Security disclosures:** private email first — see
  [`SECURITY.md`](../SECURITY.md). Finders are credited in AUDIT.md.
