# AUDIT.md — Security & spec-conformance findings

Every finding gets one row. Log while implementing, testing, and auditing — per AGENTS.md workflow step 5.

Format: `id | date | milestone | severity (high/med/low) | finding | status (open/fixed/accepted) | evidence/fix`

Severities:
- **High:** money loss/corruption, key exposure, spec guardrail (§4) or AGENTS.md Rule violated
- **Medium:** security-relevant bug, failing invariant, spec deviation with workaround
- **Low:** hygiene, docs drift, tech debt with no immediate risk

## Open findings

| id | date | milestone | severity | finding | status | evidence/fix |
|---|---|---|---|---|---|---|
| A-001 | 2026-10-14 | M0 | low | SPEC refers to the license informally as "FSL-1.1-Apache-2.0"; the official SPDX id and license abbreviation is **FSL-1.1-ALv2** (Functional Source License 1.1, ALv2 Future License) | accepted | Repo metadata (`Cargo.toml`, `package.json`) and `LICENSE` use the official SPDX id `FSL-1.1-ALv2` with the verbatim fsl.software template; SPEC header/§13 wording left untouched for the director to align (non-blocking naming ambiguity, AGENTS workflow step 9) |

## Closed findings

*(none)*
