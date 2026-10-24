# Security Policy

Tributary is a self-hosted AI token-pool gateway: it holds your members'
per-user keys, your provider API keys, and the usage metering you bill
against. We treat security reports seriously and respond to every one.

## Reporting a vulnerability

**Email: [tdhphat19@gmail.com](mailto:tdhphat19@gmail.com)** — Tran Dinh
Hong Phat, founder.

Please report vulnerabilities **privately by email only**.

- **Do NOT open a public GitHub issue** for anything security-sensitive —
  not a suspected key leak, not a bypass, not an unpatched dependency.
  Public issues about live vulnerabilities put every self-hosted
  deployment at risk; such issues will be closed on sight.
- Plain email is fine; if your report contains proof-of-concept key
  material, redact live secrets first.
- If you must use GitHub, use
  [GitHub security advisories](https://github.com/Phat1911/Tributary/security/advisories/new)
  to draft a **private** advisory — but email gets the fastest response.

### Report format

A good report includes (rough is fine — we will ask follow-ups):

1. **Affected component** — binary subcommand/route (e.g. `/v1/chat/completions`,
   `create-admin`), version/tag or commit SHA, and how you run it
   (SQLite/Postgres, behind which proxy).
2. **Impact** — what an attacker gains: key disclosure, quota bypass,
   ledger tampering, RCE, DoS.
3. **Reproduction** — step-by-step, with the exact request(s)/config. A
   curl script or test is gold.
4. **Suggested fix** — optional, but welcome.

## Scope

In scope:

- The **Tributary self-hosted gateway** as shipped in this repository's
  releases (the `tributary` binary, its migrations, the dashboard in
  `apps/dashboard`, and the packaging in `deploy/`).

Out of scope (but see below):

- Third-party dependency CVEs: report those **upstream** to the affected
  crate/package. That said, if a vulnerable version is *shipped* in a
  Tributary release (check `Cargo.lock` / `pnpm-lock.yaml`), please open a
  normal (non-security) issue or PR asking for a version bump so the fix
  lands in the next image — advisory hygiene is part of our CI
  (`cargo-deny`).
- Your own deployment's misconfiguration (exposed ports, missing proxy,
  shared master keys) unless Tributary's docs or defaults steered you
  wrong — then it's in scope as a documentation bug.
- Hosted instances we do not operate. (None exist today; there is no
  Tributary SaaS.)

## Severity expectations

**Deposits are API keys only.** A pool is funded with provider API
credits/keys — never subscription logins, never payment instruments. By
definition, **any bug that mishandles key material is critical severity**:
keys escaping encryption at rest, keys leaking into logs/traces/responses,
keys decrypted beyond their per-use boundary, or master-key exposure.
Expect such reports to be treated with maximum priority.

## Safe harbor

We are grateful for security research. If you make a good-faith effort to
comply with this policy during your research — no harm to data, no service
degradation beyond what testing minimally requires, no public disclosure
before we have had a reasonable window to fix — we will consider your
activity authorized, will **not** pursue legal action, and will work with
you to understand and resolve the issue quickly.

## Response targets

Pre-1.0 project, founder-run; these are targets, not contractual SLAs —
but we hold ourselves to them:

| Stage | Target |
|---|---|
| Acknowledge your report | **≤ 48 hours** |
| Triage + severity verdict | **≤ 5 business days** |
| Fix for criticals (key handling, RCE) | prioritized immediately; advisory + release as soon as a safe fix exists |
| Fix timeline for lower severities | communicated at triage, tracked in AUDIT.md |

## Credit

With your permission, finders are credited in **[`AUDIT.md`](AUDIT.md)**
(the repository's security-findings ledger) and in the release notes of
the first release containing the fix. Anonymous credit or no credit are
also fine — just say so.

## Supported versions

| Version | Supported |
|---|---|
| latest tagged release | ✅ |
| anything older | ❌ — pre-1.0 and fast-moving; re-test against latest before reporting, and upgrade as the fix |
| `main` branch between releases | best-effort |

---

*Deployment security notes — trusted proxies, key handling, backups — live
in [`deploy/README.md`](deploy/README.md).*
