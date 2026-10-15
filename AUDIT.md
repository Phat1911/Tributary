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
| A-002 | 2026-10-15 | M1 | low | sqlx 0.9 `Any` accepts connection URLs only; its URL round-trip cannot express absolute Windows paths (drive colon parses as a port separator, mangling `C:\…` into `sqlite://C/\…`), and relative paths do not resolve from the UNC test cwd under the Windows-cargo interop | accepted | Local (Windows-interop) tests use a single-connection in-memory SQLite pool (`db::connect_sqlite_memory`); CI on Linux exercises the real tempfile path. Product runtime on Linux/Docker is unaffected. Revisit if a Windows-hosted deployment ever matters |
| A-003 | 2026-10-15 | M1 | low | After a streamed response has begun, a `usage_events` insert failure can only be logged (response bytes already sent) — one unmetered request is possible per such failure | accepted | Buffered (non-streaming) path fails the request loudly instead (500) so metering loss is visible; streaming path logs `error!`. Detection: any `usage insert failed after stream` log line. M2+ can add a retry/outbox if this fires in practice |

## Closed findings

| A-001 | 2026-10-14 | M0 | low | SPEC referred to the license informally as "FSL-1.1-Apache-2.0"; official SPDX id is **FSL-1.1-ALv2** | fixed | Repo metadata and `LICENSE` already used the official id; SPEC header/§13/D11 aligned to `FSL-1.1-ALv2` by the director, 2026-10-14 |
