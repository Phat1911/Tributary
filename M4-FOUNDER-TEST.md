# M4 founder test checklist — leak prevention (2026-10-19, amended D31)

> **Removed 2026-10-19 (D31):** honey keys + canary model aliases were
> pulled after founder testing — low-yield tripwires (see AUDIT A-009).
> The original T1/T2 suites are gone from this list. Free-tier detection
> is now: quota caps (audited), impossible-travel (geo), classifier flags.

Everything below is testable by hand, no DB access needed. Two levels:
the **journey script** (automated, 5 min) and the **manual list** (you drive
the dashboard + curl). Expected result is stated for every step — anything
that deviates is a finding, tell the worker session.

> Windows note: the server is a Windows exe, so its `127.0.0.1:8080` is
> **Windows** loopback. Run the server, mocks, curl, and the dashboard all
> from the Windows side (PowerShell). WSL→Windows loopback is firewalled.

---

## Level 0 — automated journey script (run this first)

**Which python matters.** The server, mocks and loopback must all live on
one side of the WSL/Windows fence:

- from **WSL** (repo root): `/mnt/c/Python312/python.exe scripts/manual_journey_m4.py --ci`
- from **PowerShell**: `python scripts\manual_journey_m4.py --ci` (Windows python)
- plain `python3` in WSL = the miniforge **Linux** python → the script
  stops with a hint (no Linux cargo on this box); real Linux/CI is fine.

Expect a ✓ tree ending in `DONE — all four journeys + the shape directive
hold on this build`, exit code 0. Uses Postgres `journey_m4` (needs the
`tributary-pg-test` docker container running) — the script recreates it.

## Level 1 — boot a stack to test against by hand

```powershell
docker start tributary-pg-test
docker exec tributary-pg-test psql -U tributary -d postgres -c "DROP DATABASE IF EXISTS m4test;"
docker exec tributary-pg-test psql -U tributary -d postgres -c "CREATE DATABASE m4test;"

# build + bootstrap (any random hex string as master key — keep it for restarts)
cargo build -p tributary-gateway
$env:TRIBUTARY_DATABASE_URL="postgres://tributary:tributary@127.0.0.1:5433/m4test"
$env:TRIBUTARY_MASTER_KEY="<64 hex chars>"
# OPTIONAL real email proof (else alerts are logged as fallback lines):
# $env:TRIBUTARY_SMTP_HOST="smtp.gmail.com"; $env:TRIBUTARY_SMTP_USER="you@gmail.com"; ...
.\target\debug\tributary-gateway.exe create-admin --email root@t.dev --password hunter2hunter2
.\target\debug\tributary-gateway.exe serve        # window 1

python scripts\mock_jev.py 8098                    # window 2 (classifier mock)

cd apps\dashboard; pnpm install; pnpm dev          # window 3 → http://localhost:3000
```

Login `root@t.dev` / `hunter2hunter2`. Provision through the UI: org →
pool → provider key (base URL `http://127.0.0.1:8098`, key `sk-mock`)
→ member (password you'll remember) → issue the member a pool key —
**copy the raw key from the reveal-once dialog**.

---

## T1 — purpose classifier (J3)

- **T1.1 Probe-on-save**: Organization → Purpose classifier. Save with a
  wrong `base_url` → EXPECT **rejected, nothing stored** ("classifier
  probe failed"). Save with `http://127.0.0.1:8098`, model `jev-align-1`,
  a purpose statement → saved, mode ALERT.
- **T1.2 Member disclosure (Alert mode already!)**: login as the member →
  My keys → disclosure banner about monitoring is visible. Log out.
- **T1.3 Off-purpose request**: member key, message containing the marker
  `OFF-PURPOSE` → **200 served** (alert mode), then a review flag appears
  carrying a **score** — open the flag: it must contain NO message text
  (score, model, prefix only).
- **T1.4 Auto-block gating**: switch config to auto-block WITHOUT the
  acknowledge checkbox → EXPECT 400 warning. WITH ack → saved in
  **SHADOW**. Send OFF-PURPOSE twice → shadow stats count
  "would have blocked". Key still works (shadow never acts).
- **T1.5 Go live**: press go-live (explicit). Send OFF-PURPOSE. Within a
  second the member key now answers **403 suspended pending review** with
  the recovery hint; member's My-keys page says pending review; the
  disclosure banner text strengthens.
- **T1.6 One-click unblock**: Review queue → *Unblock* → the same key
  serves **200** again immediately.

## T2 — IP allowlist (J4)

- **T2.1 Pin**: Keys tab → Manage → allowlist editor → add `203.0.113.0/24`.
- **T2.2 In-range served**: add header `X-Forwarded-For: 203.0.113.5` to
  a normal chat request → **200**.
- **T2.3 Outside denied**: same request without the header (or
  `X-Forwarded-For: 198.51.100.9`) → **403** "restricted to configured IP
  ranges", and an audit row `security.ip_allowlist_denied` appears.
  No review-queue item (by design — audit-only).
- **T2.4 Fail-closed check**: request with NO proxy header = direct/local
  source → **denied** (a direct connection cannot prove it's in-range).
- **T2.5 Dormant again**: delete the CIDR → requests with no header serve
  normally.

## T3 — key governance extras

- **T3.1 Rotate**: Manage → Rotate → raw new key shown **once**; old key
  immediately 401s; prefix changes in listing.
- **T3.2 Expiry**: set expiry in the past → key rejected.
- **T3.3 Review queue actions**: Dismiss and Revoke each do what they say
  (dismiss keeps the key; revoke kills it).

## T4 — quota cap fires visibly (amended M4 done-when)

- **T4.1 Set a tiny cap**: pool → Budgets → day cap of `10` tokens.
- **T4.2 Spend it**: two requests with the member key; the second must
  return **429** (never touch the provider).
- **T4.3 Audit proof**: Audit log shows a `security.quota_denied` row —
  a cap firing is demonstrable, not an outage in disguise.

## What is knowingly NOT testable by hand right now

- Impossible travel — needs the geoip feature (off) + a GeoLite2 db.
- Concurrent-IP check — not built (PLAN gap, M5 candidate).
- SMTP failure modes beyond fallback logging.
