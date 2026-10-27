# Pilot support runbook — debugging a machine you can't see

> For the founder (or whoever takes the support call). A pilot says "it
> broke" on the phone; you have no access to their machine. Everything
> below is **read to me over the phone**: every command is safe, changes
> nothing, and fits in one line. Work top to bottom — the ladder is
> ordered by how often each step has explained a real report.

**Ask once, up front:** "What did you last change, and when?" (an
upgrade, a new member, a budget, a firewall tweak). Half of all reports
end here.

## Step 0 — what are they even running?

```sh
docker compose images
```

Read the **TAG** column to you. If it says `latest`, they don't know
their version — that itself is a finding (see the [stale image](#fix-3)
fix below). No `docker compose`? See [fix 2](#fix-2).

## Step 1 — is the stack up? (`healthz`)

Ask them to run this **on the machine hosting Docker** (or paste the URL
in a browser):

```sh
curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8111/healthz
```

- **200** — the gateway and its database are alive. The problem is
  higher up: a member's request, a key, a quota. Go to step 3.
- **503** — the process answers but the database doesn't. Go to step 2
  and look for migrate/DB errors.
- **connection refused / timeout** — nothing is listening. Go to step 2.
- Refused **from a member's machine but 200 on the host** — network, not
  Tributary: which [sharing rung](../deploy/README.md#sharing-ladder--who-can-reach-your-instance)
  are they on? LAN: firewall on the host (`sudo ufw status`). Tailnet:
  is Tailscale up on both ends, and did they use the `https://…ts.net`
  URL for dashboard logins (the cookie needs HTTPS away from localhost)?
  VPS: is their reverse proxy running?

## Step 2 — what does the container say? (`docker logs`)

```sh
docker ps --filter name=tributary
docker logs --tail 200 tributary
```

Read to you, line by line. What you're listening for:

- **`Bind error` / `Address already in use`** — port clash on the host:
  [fix 1](#fix-1).
- **Migration errors** (`migrate` in the message, SQL errors) — a
  database/schema problem; STOP, get the backup story before touching
  anything, and check what [version](#step-0--what-are-they-even-running)
  they're on versus what they were on.
- **Panics / repeated restarts** (`docker ps` shows `Restarting`) —
  note the panic line verbatim; that's a bug report, not a phone fix.
- Anything about the **master key** (64-hex complaints) — they changed
  `.env`; provider keys are now undecryptable. Restore the old key or
  re-enter provider keys.

## Step 3 — what did the system itself record?

Two surfaces, both in the dashboard (admin login):

1. **Audit log** (`Audit log` in the sidebar): every admin action with a
   timestamp — member added, key issued/revoked, budget set, provider
   key sealed. If the pilot says "nobody touched anything", this is the
   truth check. Ask: "read me the last three rows."
2. **Review queue** (`Review queue`): every detection — off-purpose
   requests, auto-blocks (a **suspended key stops all requests with a
   429**), country changes, key-shared-across-networks flags. If a
   member is suddenly blocked, it's usually a row here, with a Resolve
   action (unblock / revoke / dismiss) next to it.

**Member-specific 429s** (the member can work, one can't): quota. Ask
the admin — Budgets tab on the pool — "what rules exist for this member,
and what window?" Remember the semantics from
[the deploy guide](../deploy/README.md#quota-behavior--how-requests-are-counted):
denials at **reserve time** quote an *estimate* (prompt chars ÷ 4 +
max_completion_tokens, 256 assumed if undeclared) and the message says
so; usage-cap denials read `Limit reached: …`. Budget changes take
**up to ~5 seconds** to reach the gateway (rules are cached briefly) —
set, wait five seconds, retry.

> **Pre-0.1.3 defect (fixed in v0.1.3):** on v0.1.2 and older, a budget
> created with the **"Minute (requests)"** window was stored as a *token*
> cap of the same number — "5 requests/minute" became "5 tokens/minute",
> denying **every** request at reserve time. If a member went from
> working to all-429 right after someone set a minute budget: check their
> [version](#step-0--what-are-they-even-running) — on an old image,
> [upgrade](../deploy/README.md#upgrading) or delete the minute rule,
> wait 5 seconds, retry. On 0.1.3+ the window is a true request cap (the
> 6th request in a minute 429s with `Limit reached: N requests per
> minute`). See the [onboarding guide](onboarding-org-admin.md#8-set-a-budget).

## Step 4 — the known fixes

<a id="fix-1"></a>

### Fix 1 — port conflict ("container won't start, something about the port")

Symptom: `docker ps` shows no healthy container; logs say the port is
taken. Something else on the host owns the published port.

Fix: change the **left** side of `ports:` in `docker-compose.yml`
(e.g. `"8111:8080"` → `"8112:8080"`), then `docker compose up -d`.
The container side stays `8080` always. Details:
[Ports — read this if you already run something on 8080](../deploy/README.md#ports--read-this-if-you-already-run-something-on-8080).

<a id="fix-2"></a>

### Fix 2 — `permission denied ... /var/run/docker.sock`

Their user isn't in the `docker` group. Fix:
`sudo usermod -aG docker $USER && newgrp docker`, then check `docker ps`
runs without sudo. WSL specifics: [the README quickstart callout](../README.md#quickstart).

<a id="fix-3"></a>

### Fix 3 — stale image ("we upgraded but nothing changed" / weird old bugs)

They ran `docker compose up -d` **without** `pull`. With the shipped
`pull_policy: missing`, that never fetches a newer image — they're still
on whatever they first pulled. Fix + how to never see it again (pin the
tag): [Upgrading](../deploy/README.md#upgrading).

<a id="fix-4"></a>

### Fix 4 — `store:false` agents 400 on Gemini ("Unknown name store" from Google)

Agents that send `store: false` with every request get **400s from
Google's compat endpoint on any image before v0.1.2** (D37). Fix:
upgrade to `0.1.2` or later — the gateway drops the negated field for
gemini-labeled keys. Check their [version](#step-0--what-are-they-even-running)
first. Background: [Field drops (D37)](../deploy/README.md#provider-keys--openai-and-gemini).

<a id="fix-5"></a>

### Fix 5 — WSL / Docker Desktop ghosts

Two Docker engines or two Tailscales on one machine produce "works in
one terminal, not the other" symptoms. Fix: one of each —
[One machine, one of each](../deploy/README.md#one-machine-one-of-each-read-this-if-you-use-wsl-or-docker-desktop).

## When it's actually a bug

You've walked the ladder, the fix table didn't catch it, healthz is 200
but behavior is wrong (or the logs show a panic): that's a finding, not
a phone fix. Ask the pilot to file it through the
[structured feedback form](https://github.com/Phat1911/Tributary/issues/new?template=pilot-feedback.yml)
— version + rung + steps + the `docker logs` excerpt they just read you
— and it lands in the queue with everything needed to reproduce.
