# Member keys table — expiry + cap surfacing (pre-M5 chore, 2026-10-22)

Manual journey for the two new columns in **My pool keys** (`/me`):
**Expires** and **Cap**. This is a surfacing chore — nothing here changes
enforcement; the gateway already rejected expired/exhausted keys before
this change.

Boot a stack exactly as in `M4-FOUNDER-TEST.md` Level 1 (server + Postgres
`m4test` + dashboard). A provider key is only needed for step K4 (making
`used` move); every other step is control-plane only.

## K1 — login as a member, see both columns

Log in as the member (not the admin) → **My keys & usage**. The keys table
now shows **Expires** and **Cap** between *Issued* and the revoke button —
same date format as *Issued* ("Oct 22, 2026, 9:41 PM").

## K2 — Never + Unlimited (the uncapped default)

Ask the admin to issue you a key with **no expiry and no budget** (the
default everywhere), or use a fresh member with a plain issued key.

- **EXPECT** Expires shows **Never** (muted).
- **EXPECT** Cap shows **Unlimited** (muted).

## K3 — a capped, expiring key

As the admin: pool page → **Budgets** → set a *member-scoped* (or key)
**day** cap, e.g. `100000` tokens → then keys tab → **Manage** → expiry
editor → set expiry ~1 month out. Back on the member's `/me`:

- **EXPECT** Cap shows `0 / 100k · day` (compact form; the `· day` is the
  window the numbers are counted in — hour/day/month).
- **EXPECT** Expires shows the date you set (plain, no tint).

## K4 — the number is real usage

Point a client at the gateway with the member key, run one request
(mock upstream is fine). Refresh `/me`:

- **EXPECT** Cap's left number moved up by the request's tokens
  (e.g. `27 / 100k · day`). This is the same SUM the gateway enforces —
  when it hits the limit the request 429s (T4 in the M4 list).

## K5 — warning tint and the expired chip

As the admin, set the key's expiry to **6 days** out → member `/me`:
- **EXPECT** the Expires date turns **amber** (< 7 days left; still
  readable in both themes).

Set expiry to **1 minute** out, wait, refresh:
- **EXPECT** an **expired** chip next to the date, while the **Status**
  column still says `active` — expiry is display-level + gateway
  enforcement, never a stored lifecycle state (that's why there is no
  "expired" status anywhere).

## K6 — dead keys show a dash

Revoke that key (member self-revoke or admin revoke) → refresh:

- **EXPECT** Status flips to `revoked` and Cap shows **—** (nothing is
  enforceable on a dead key); Expires keeps its last known value.

## What is knowingly NOT covered by hand

- `hour` / `month` windows and cross-subject resolution (a user-level cap
  beating a pool-level one) — proven by the automated dual-DB journey
  `member_key_list_surfaces_expiry_and_effective_cap`
  (`crates/control/tests/self_service.rs`), which the founder does not
  need to drive by hand.
