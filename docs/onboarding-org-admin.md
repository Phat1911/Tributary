# Onboarding an org admin — first day, empty box to first request

> **DRAFT v1 (M6 slice 1).** Verified step-by-step against a **live
> v0.1.2 stack** (released image, dashboard it serves). The "first
> request" leg was verified with a **mock OpenAI-compatible upstream**;
> real-provider steps get confirmed by pilots during M6 and this guide is
> rewritten from their friction (slice 2). Found something that fights
> this guide? [File it](https://github.com/Phat1911/Tributary/issues/new?template=pilot-feedback.yml) — that's how the next version gets written.

## Who this is for

Two roles, one box — read this once so the handoff makes sense:

- **Host admin** — you deployed the container and created the first
  login (the `create-admin` step in the
  [quickstart](../README.md#quickstart)). You own the machine, the
  backups, and upgrades. You mint org admins; you do not run the org
  day-to-day.
- **Org admin** — the person (often you, wearing a different hat) who
  manages one organization's members, pools, and budgets. This is the
  role a pilot org's lead lives in.

The whole first day is ~15 minutes: **Part 1** (host, once per org) is
3 steps, then **Part 2** (org admin) is the working loop.

## Part 1 — host admin: create the org and its admin

Log in at your instance's URL (default `http://localhost:8111`). You
land on the **host console** — every organization and pool on the
deployment at a glance.

### 1. Create the organization

**New organization** → fill **Name** and **Slug** (lowercase, e.g.
`pilot-works`) → **Create**. The org appears in the table.

### 2. Mint the org's first admin

On the org's row: **Add admin** → email + a temporary password (min. 8
characters) → **Create org admin**.

This is deliberate escalation design: only the *host* can mint org
admins; org admins can only create plain members. Hand the credentials
over a secure channel, and have them change nothing yet — read Part 2
with them once, then let them drive.

### 3. Hand over the URL

The org admin signs in at the **same URL** you use — the dashboard
routes them to their org's console automatically.

## Part 2 — org admin: the working setup

Sign in; you land on **Organization** — the roster (members) and your
pools. The sidebar is your surface: Organization, Review queue, Purpose
classifier, Detection settings, Audit log. This part walks the four
things every org needs, in the order that works: **a member, a funded
pool, a key, a budget** — then the first request.

### 4. Add the first member

**Add member** → email + temporary password → **Add member**. They
appear in the roster. Members sign in to see their own usage; for API
traffic all they need is the key you issue in step 7.

### 5. Create the pool

**New pool** → **Name** (e.g. `default`) → **Model allowlist**.

The allowlist is comma-separated model ids the pool will serve — it must
match what your provider key actually serves. **Verify, never guess**
(ids drift): enumerate your endpoint with the models curl in the
[deploy guide's provider section](../deploy/README.md#provider-keys--openai-and-gemini),
then paste what it reported (e.g. `gpt-4o-mini`, or your verified
`gemini-*` id). Leave it empty only if you mean "the deployment
default".

### 6. Fund it: add the provider key

Open the pool (click its name) → **Provider keys** tab → **Add provider
key**:

- **Provider**: OpenAI, Gemini (AI Studio key), or Custom
  (OpenAI-compatible — anything else, including self-hosted).
- **API key**: pasted once — it is envelope-encrypted before storage and
  only a masked prefix is ever shown again.
- **Base URL**: prefilled per provider; for Custom, the endpoint's `/v1`
  root.

**Add & validate** probes the endpoint immediately — a green **ok**
health badge means the key actually works; `invalid` means check the key
or the base URL before blaming anything else. Honest limits pass through
verbatim: free-tier Gemini keys are rate-limited by Google and will
surface upstream 429s under load ([rate-limit honesty](../deploy/README.md#provider-keys--openai-and-gemini)).

### 7. Issue the member's pool key

**Keys** tab → **Issue key** → pick the member → **Issue key**.

The raw key (`sk-pool-…`) shows **exactly once**, behind an explicit
reveal toggle — copy it and hand it over a secure channel. It is stored
only as a hash; there is no "show it again". From the key's row you can
later rotate it, set an expiry, pin an IP allowlist, or revoke it.

### 8. Set a budget

**Budgets** tab → **Set budget**:

- **Applies to**: *Whole pool* or *One member* (start with one member —
  that's the per-user cap the product is about).
- **Window**: **Day** or **Month** for token budgets; **Minute
  (requests)** is a request-rate throttle.
- **Limit**: the number for the window — tokens (e.g. `100000`) for
  day/month, requests (e.g. `5`) for minute.

Budgets are enforced at the gateway **before any upstream call** — a
denied request costs no provider tokens and the 429 explains itself
(estimate vs. cap shapes:
[quota behavior](../deploy/README.md#quota-behavior--how-requests-are-counted)).
Two things pilots trip on:

- **Changes take up to ~5 seconds** to reach the gateway (rules are
  briefly cached). Set, count five, retry.
- **Minute windows throttle requests** (true request cap since **v0.1.3**):
  "5" means 5 requests per minute — the 6th request inside a minute gets a
  429 reading `Limit reached: 5 requests per minute`. **On v0.1.2 or older
  this window was defective**: it stored the number as a *token* cap, so
  "5 requests/minute" denied every request outright. If you're on an old
  image: [upgrade](../deploy/README.md#upgrading), or delete the minute
  rule and wait five seconds — the member recovers immediately.

### 9. First request

From any machine that can reach the instance, with the member key from
step 7 — the pool is an OpenAI-compatible endpoint:

```sh
curl -s http://localhost:8111/v1/chat/completions \
  -H "Authorization: Bearer sk-pool-…" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Say hi"}]}'
```

(Substitute your instance URL, a model from your pool's allowlist, and
the member key.) Any OpenAI SDK works the same way — set
`base_url` to `http://<your-instance>:8111/v1` and `api_key` to the
member key. A 200 with a completion is success. On a failure, read the
error body: **429s explain themselves** — a budget or rate decision
names the window and the limit (estimate vs. cap shapes in step 8), and
quota denials happen **before** any upstream call, so a denied request
costs no provider tokens.

### 10. See it metered

**Usage** tab on the pool: tokens per day (14-day sparkline), a
per-member filter, and the event rows. This is also your "did it really
work" check — every successful request leaves a row.

### 11. Review-queue orientation (nothing to do yet)

**Review queue** in the sidebar. Empty is the healthy default — it is
not a TODO list, it's where *detections* land: off-purpose requests (if
you configure the classifier), auto-blocks, country changes, key-shared-
across-networks flags; plus pending sensitive-action approvals once you
have a second admin. When a row appears, it has context and three
responses (unblock / revoke / dismiss) — detection is automatic, the
response is a human decision. The **Audit log** beside it records every
admin action, yours included.

## Where to go next

- Operations — ports, sharing (LAN / tailnet / VPS), backups, Postgres,
  upgrades: [`deploy/README.md`](../deploy/README.md)
- Something fought you? [Pilot feedback form](https://github.com/Phat1911/Tributary/issues/new?template=pilot-feedback.yml)
  — steps + logs, it feeds the rewrite of this guide.
