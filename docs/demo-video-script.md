# Demo video script — Tributary in 2½ minutes (M5.5)

A scene-by-scene script for a founder-recorded video. Target length
**2:00–3:00**. Every scene states the exact UI state it needs, so the
stack can be pre-staged before recording; nothing on camera is faked —
every click is a real flow, every flag was raised by real traffic.

**Recording setup (do this once):**

- Browser: clean profile, **1440×900 window**, dark OS theme (the
  product is dark-only — that is by design).
- Terminal font bumped up (16–18 pt) — it shares the screen.
- Record one scene at a time; the final cut is just scenes in order.
  One-take-per-scene is fine — nobody expects live narration perfection.
- The stack: the staged demo instance on **http://localhost:8111**
  (see "Pre-stage" at the bottom). If it is not running, re-stage it
  first — commands included there.

**Narration style:** short lines, one breath each. The SAY lines below
are written for a non-native read — if a line feels awkward in your
mouth, say the same thing in your own words. Slower is better than fast.

---

## Timing budget

| # | Scene | Seconds |
|---|-------|---------|
| 1 | One container up | 0:00–0:15 |
| 2 | Login | 0:15–0:30 |
| 3 | The pool | 0:30–0:55 |
| 4 | A member key, shown once | 0:55–1:20 |
| 5 | Quotas that hold | 1:20–1:40 |
| 6 | The two-network flag | 1:40–2:05 |
| 7 | The audit log | 2:05–2:25 |
| 8 | Close — free, self-hosted | 2:25–2:40 |

---

## Scene 1 — One container up (terminal)

**ON SCREEN:** terminal, the five quickstart lines visible (pasted, not
typed live), then `docker ps` showing `tributary` healthy, then
`curl -si localhost:8111/healthz` → `HTTP/1.1 200 OK` (empty body —
the status line IS the health signal).

**DO:** run `docker ps` and the curl; let the `200 OK` sit on screen a
beat.

**SAY:**
> "Tributary is one container. Clone, set one key, compose up.
> About two minutes to a login."

**PRE-STAGE:** stack running (`docker ps` shows `tributary` Up); the
clone/cp/echo/compose lines pasted in the terminal scrollback.

---

## Scene 2 — Login (browser)

**ON SCREEN:** http://localhost:8111/login, empty form.

**DO:** type `avery@labs.example` / `AveryLaunch-Demo2026`, click
**Sign in**. Land on the org console.

**SAY:**
> "Log in as the org admin. First stop: the org console —
> your members and your pools."

**PRE-STAGE:** none beyond the stack (fresh login page: log out first,
or use a clean browser profile).

---

## Scene 3 — The pool (org console → Team Pool)

**ON SCREEN:** org console — the member roster (5 people) and the
pools table (Team Pool, active).

**DO:** click the **Team Pool** link. The pool workbench opens on the
Keys tab: one masked key per member (`sk-pool-…`), statuses visible —
casey **active**, devon **suspended**, quinn **rotated** + a new
**active** key.

**SAY:**
> "One pool holds the real provider key — encrypted at rest.
> Every member gets their own key instead.
> Rotate, suspend, expire — all from here."

**PRE-STAGE:** seed run completed (the three key states ARE the story —
they came from real events: suspension by auto-block, a rotation).

---

## Scene 4 — A member key, shown once (pool workbench)

**ON SCREEN:** pool workbench, Keys tab.

**DO:** click **+ Issue key**, pick **avery** (an admin without a key
yet), confirm. The raw key appears in a one-time dialog — point at it,
then close the dialog WITHOUT copying.

**SAY:**
> "Issue a key: the raw value appears exactly once — then never again.
> Close the dialog, and it stays secret."

**PRE-STAGE:** avery must NOT already have a key (default staged state:
only casey/devon/quinn do). If re-staged after a rehearsal where you
issued one, revoke avery's key first (trash icon) — it is demo data.

**NOTE:** this is the only moment a raw key is on camera. It is a
throwaway key on a localhost demo stack pointing at a mock upstream —
worthless outside this machine. Say so if asked.

---

## Scene 5 — Quotas that hold (Budgets tab)

**ON SCREEN:** pool workbench → **Budgets** tab: three rules —
quinn's key 40 tokens/day, devon's key 200,000/month, the pool
2,000,000/month.

**DO:** click the **Budgets** tab; let the rules sit on screen.

**SAY:**
> "Budgets attach to a key, a person, or the whole pool.
> Quinn's key is capped at forty tokens a day.
> A request over budget is rejected BEFORE the provider —
> so it never costs you anything."

**PRE-STAGE:** seed run (the three rules). If asked on camera how a
denial looks: it is in the audit log — next scenes.

---

## Scene 6 — The two-network flag (Review queue)

**ON SCREEN:** **Review queue** — two open detections:
**Auto-block (key suspended)** (high) and
**Key used from multiple networks** (medium, detail line shows two
`/24` networks).

**DO:** click **Review queue** in the sidebar. Point at the
multiple-networks row, then at the **Unblock key** button on the
auto-block row (do not click it).

**SAY:**
> "Casey's key was used from two different networks — that lands here.
> Auto-block only suspends a key and waits for a person.
> Detection never silently bans anyone.
> One click: unblock, revoke, or dismiss."

**PRE-STAGE:** seed run — the D32 flag and the auto-block flag are open
by default. If the queue shows a third row ("Off-purpose request",
medium) — that is the live-catch evidence row; it is fine to leave on
camera or dismiss it before recording (Dismiss).

---

## Scene 7 — The audit log

**ON SCREEN:** **Audit log** — page 1 full of rows:
`security.quota_denied` (the 429), `pool_key.rotated` (quinn),
`approval.requested` / `approved` / `executed` (the four-eyes chain).

**DO:** click **Audit log** in the sidebar; slowly hover down the rows.

**SAY:**
> "Everything is on the record: who, what, when — from where.
> The quota denial. The rotation. And the approvals —
> sensitive actions need a second admin here."

**PRE-STAGE:** seed run (all rows present on page 1).

---

## Scene 8 — Close (login page)

**ON SCREEN:** sign out (bottom of sidebar) → the login page, clean.

**SAY:**
> "Self-hosted. Free — all of it.
> One container, your keys, your rules.
> Tributary — small streams, pooled power."

**DO:** end recording. Optionally show the repo URL for three seconds:
`github.com/Phat1911/Tributary` (a browser tab title is enough).

---

## Pre-stage — making the stack look like this script

The staged instance (fictional "Example Labs" org, mock upstream, all
flags raised by REAL served traffic) is built by the repo's own rig.
From the repo root on the recording machine:

```sh
# 1. Stack up (if not already running) — see deploy/README.md quickstart,
#    or the compose-free path:
docker run -d --name tributary -p 8111:8080 \
  -e TRIBUTARY_MASTER_KEY=$(openssl rand -hex 32) \
  -v tributary-data:/data --restart unless-stopped \
  ghcr.io/phat1911/tributary:latest
docker exec -e TRIBUTARY_DATABASE_URL=sqlite:///data/tributary.db \
  tributary tributary create-admin \
  --email host@tributary.example --password 'HostLaunch-Demo2026'

# 2. The mock upstream + the D32 second network need a docker network:
docker network create --subnet 172.28.0.0/16 demo-net
docker network connect --ip 172.28.0.3 demo-net tributary
docker run -d --name demo-upstream --network demo-net \
  -v "$PWD/scripts/mock_jev.py:/mock_jev.py:ro" --restart unless-stopped \
  python:3.13-alpine python3 /mock_jev.py 8098 0.0.0.0

# 3. Stage the org + stories (public API only, idempotent-fresh):
python3 scripts/seed_post_demo.py
```

Logins it prints: `avery@labs.example / AveryLaunch-Demo2026` (org
admin — the on-camera account), plus blake (approver) and the host
login. Demo-only throwaways for a localhost stack.

**Two staging lessons (recorded the hard way, 2026-10-25):** if you
hand-edit the SQLite file for usage history, copy `db`+`-wal`+`-shm`
out together (the WAL may hold today's rows) and `chown 999:999` the
file after copying back — otherwise every write fails with
"readonly database". The seed script avoids both by never touching the
file directly.

**If re-staging for a second take:** the seed script is
idempotent-fresh — easiest is a fresh volume: `docker rm -f tributary
&& docker volume rm tributary-data`, then steps 1–3 again.
