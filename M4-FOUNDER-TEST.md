# M4 founder test checklist — leak prevention (2026-10-19)

Everything below is testable by hand, no DB access needed. Two levels:
the **journey script** (automated, 5 min) and the **manual list** (you drive
the dashboard + curl). Expected result is stated for every step — anything
that deviates is a finding, tell the worker session.

> Windows note: the server is a Windows exe, so its `127.0.0.1:8080` is
> **Windows** loopback. Run the server, mocks, curl, and the dashboard all
> from the Windows side (PowerShell). WSL→Windows loopback is firewalled.

---

## Level 0 — automated journey script (run this first)

```powershell
# from the repo root, Windows python (builds the server itself)
python scripts\manual_journey_m4.py --ci
```

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

## T1 — honey key (J1)

- **T1.1 Plant**: pool → Canaries tab → *Plant honey keys* → the dialog
  shows raw honey key(s) **once** — copy. EXPECT: never shown again;
  listing afterwards shows only a masked prefix.
- **T1.2 Indistinguishable in the shared listing**: Keys tab — the honey
  row shows the same five columns as live keys (created date, prefix,
  status…), **no badge, no kind marker**, and its created date is
  interleaved among the others (not clustered at the end). Identity
  appears ONLY in *Manage keys* (amber row).
- **T1.3 Use it like a leaker** (PowerShell):
  ```powershell
  curl.exe -s -i -H "Authorization: Bearer <HONEY_KEY>" http://127.0.0.1:8080/v1/models
  ```
  EXPECT: **401 whose body is byte-identical** to a bogus key — run the
  same with `Bearer sk-pool-bogus...` and compare: same status, same
  JSON, same length.
- **T1.4 Consequences**: Review queue has a *honey key used* flag;
  audit page has a row; an email alert went out (inbox, or
  `email fallback ...` line in the server window). The honey key serves
  **zero** traffic (usage page shows nothing for it).

## T2 — canary model (J2) + enumeration pattern (new)

- **T2.1 Arm**: Canaries tab → add alias e.g. `gpt-5-turbo-preview`.
- **T2.2 Rides the listing**: as the member (or curl with the member key)
  `GET /v1/models` → alias present, shaped exactly like real models.
- **T2.3 Request it**:
  ```powershell
  curl.exe -s -i -H "Authorization: Bearer <MEMBER_KEY>" -H "Content-Type: application/json" ^
    -d "{\"model\":\"gpt-5-turbo-preview\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}" ^
    http://127.0.0.1:8080/v1/chat/completions
  ```
  EXPECT: **400** with `Model 'gpt-5-turbo-preview' is not allowed for this
  pool.` — the identical message a random model name gets.
- **T2.4 Enumeration ⚠ (the A-008 addition)**: run T2.3 two more times.
  Third+ hit ⇒ the Review queue flag gains an amber line:
  *⚠ 2 canary hits / 24h — listing enumeration pattern*. First hits say
  plain "exfiltrated". The requester never sees any difference.

## T3 — purpose classifier (J3)

- **T3.1 Probe-on-save**: Organization → Purpose classifier. Save with a
  wrong `base_url` → EXPECT **rejected, nothing stored** ("classifier
  probe failed"). Save with `http://127.0.0.1:8098`, model `jev-align-1`,
  a purpose statement → saved, mode ALERT.
- **T3.2 Member disclosure (Alert mode already!)**: login as the member →
  My keys → disclosure banner about monitoring is visible. Log out.
- **T3.3 Off-purpose request**: member key, message containing the marker
  `OFF-PURPOSE` → **200 served** (alert mode), then a review flag appears
  carrying a **score** — open the flag: it must contain NO message text
  (score, model, prefix only).
- **T3.4 Auto-block gating**: switch config to auto-block WITHOUT the
  acknowledge checkbox → EXPECT 400 warning. WITH ack → saved in
  **SHADOW**. Send OFF-PURPOSE twice → shadow stats count
  "would have blocked". Key still works (shadow never acts).
- **T3.5 Go live**: press go-live (explicit). Send OFF-PURPOSE. Within a
  second the member key now answers **403 suspended pending review** with
  the recovery hint; member's My-keys page says pending review; the
  disclosure banner text strengthens.
- **T3.6 One-click unblock**: Review queue → *Unblock* → the same key
  serves **200** again immediately.

## T4 — IP allowlist (J4)

- **T4.1 Pin**: Keys tab → Manage → allowlist editor → add `203.0.113.0/24`.
- **T4.2 In-range served**: add header `X-Forwarded-For: 203.0.113.5` to
  a normal chat request → **200**.
- **T4.3 Outside denied**: same request without the header (or
  `X-Forwarded-For: 198.51.100.9`) → **403** "restricted to configured IP
  ranges", and an audit row `security.ip_allowlist_denied` appears.
  No review-queue item (by design — audit-only).
- **T4.4 Fail-closed check**: request with NO proxy header = direct/local
  source → **denied** (a direct connection cannot prove it's in-range).
- **T4.5 Dormant again**: delete the CIDR → requests with no header serve
  normally.

## T5 — key governance extras

- **T5.1 Rotate**: Manage → Rotate → raw new key shown **once**; old key
  immediately 401s; prefix changes in listing.
- **T5.2 Expiry**: set expiry in the past → key rejected.
- **T5.3 Review queue actions**: Dismiss and Revoke each do what they say
  (dismiss keeps the key; revoke kills it).

## What is knowingly NOT testable by hand right now

- Impossible travel — needs the geoip feature (off) + a GeoLite2 db.
- Concurrent-IP check — not built (PLAN gap, M5 candidate).
- SMTP failure modes beyond fallback logging.
