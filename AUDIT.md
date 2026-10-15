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
*(none)*

## Closed findings

| A-001 | 2026-10-14 | M0 | low | SPEC referred to the license informally as "FSL-1.1-Apache-2.0"; official SPDX id is **FSL-1.1-ALv2** | fixed | Repo metadata and `LICENSE` already used the official id; SPEC header/§13/D11 aligned to `FSL-1.1-ALv2` by the director, 2026-10-14 |
