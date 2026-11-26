# Founder manual journey — the Settlement screen (SCREEN-1)

**What this proves:** the org admin's settlement screen for a root pool
(M9, SPEC D43) — the books (who contributed, burned and earned) and the
"Settle last hour (8%)" run that pays the depositors. It is a UI-visible
change, so a manual journey ships with it (workflow step 12). **About
10 minutes**, once the image exists.

Two parts: **Part A** on your demo stack with your real Gemini key (what
the recording uses); **Part B** a clean-room repeat that needs no key and
has fixed, predictable numbers. Run A; run B if anything in A surprises
you. Report format at the bottom.

## Read this first — three traps

1. **You must be an ORG admin to see the link.** `create-admin` makes a
   HOST admin, and every `/org/*` page — this one included — is hidden
   from host admins. If the sidebar has no **Settlement** entry, that is
   why; step A1 is the fix. (The page itself works for a host admin at its
   direct URL `/org/settlement/`; only the link is gated.)
2. **A wrong `base_url` is not a harmless typo.** The gateway appends
   `/chat/completions` to `base_url`, so it must end in the version path
   (`…/v1`, `…/v1beta/openai`) with **no trailing slash**. If it is wrong
   the upstream answers an error — and, by design (the A-053 floor),
   **each failed request still burns the reserve estimate** from the
   depositor's credits ($0.03855 per failed request in this session's
   runs; it depends on the prompt and `max_tokens`). Symptom on this
   screen: *Burned* climbs while every request fails.
3. **One provider name per family.** A request resolves to ONE bucket.
   When two depositors declare the same family under different
   `provider` names, the alphabetically smaller name takes 100% of that
   family's traffic; the others never burn, so never earn (AUDIT A-061,
   open). For a demo where weights should matter, give every deposit the
   **same `provider` string**.

## 0. Get the page into an image

The dashboard is static files **baked into the gateway image**
(`deploy/Dockerfile` stage 1 → `/usr/share/tributary/dashboard`). Nothing
but a new image makes the page appear: a restart, re-pulling an older
tag, or `pnpm build` on the host changes nothing a running container
serves.

From a checkout of `main` at or after the SCREEN-1 commit — find it with
`git log --oneline -1 -- apps/dashboard/app/org/settlement/page.tsx` —
and never `docker build` from a folder a session is editing:

```bash
TAG=tributary:screen1
git archive main | docker build -f deploy/Dockerfile -t $TAG -
```

That is a full Rust release build: expect tens of minutes the first time.
Confirm the page really landed before going further:

```bash
docker run --rm --entrypoint sh $TAG -c 'ls /usr/share/tributary/dashboard/org/settlement/index.html'
```

It prints the path on a new image, and `No such file or directory` on
`tributary:m9exp` or any older tag — which is the point of the check.

## Part A — on your demo stack (real Gemini key)

Use the stack you raise for the recording. It already holds your org, a
root pool and a Gemini deposit; if you need to recreate them, the calls
are in Part B with the runbook §0's verified values (`provider
"gemini"`, family `"gemini"`, `base_url
https://generativelanguage.googleapis.com/v1beta/openai`, no trailing
slash).

| # | Do | Expect | PASS when |
|---|----|--------|-----------|
| A1 | Sign in as your `create-admin` account → **Instance** → the org that owns the root pool → **Add admin** (an email + a password of 8+ chars) | Toast: *… is now <org>'s admin* | You have an ORG-admin email and password |
| A2 | **Sign out**, then sign in as that org admin | Sidebar lists **Settlement**, between Organization and Review queue | The link is there |
| A3 | Click **Settlement** | Page titled *Settlement*; **Root pool** picker shows your root pool; one row per depositor × family | Each row's Contributed equals that deposit (hover shows the exact micro-USD) |
| A4 | Send one request through the pool with a member key, then click **⟳** on the Depositors card | *Burned* rises by that request's cost | Burned > 0 and Earned still $0.00 |
| A5 | Click **Settle last hour (8%)** | Under the button: the window, four figures, a Payouts table | *Distributable = Fee + Paid out + Undistributed*, exactly |
| A6 | Look back at the table — do not refresh | Each paid depositor's **Earned** grew by its payout | Footer Earned = Paid out; Contributed and Burned unchanged |
| A7 | Send another request, then click **Settle** again | A NEW window (a later `from_ms`); only the new burn is paid | The earlier payout is still in Earned; nothing is paid twice |
| A8 | Copy `from_ms` and `to_ms` off the screen and replay the run by hand (curl: Part B, step B8) | **The same JSON the screen showed** | Identical — an identical window replays its first answer |

Numbers will differ from Part B's (a real model prices differently). The
relationships in the PASS column are what must hold.

## Part B — clean-room repeat, no key needed

Reproducible from the repo alone: the gateway image plus the repo's mock
upstream, so every number below is fixed. Run from the **repo root**, in
one shell, blocks in order. Every port avoids 8111 / 8112 / 8113; every
name starts with `screen-journey-`; the last block removes everything.

### B1 — raise a throwaway stack

(`TAG` comes from section 0 — it is set there, once.)

```bash
PORT=8147; GW=screen-journey-gw; MOCK=screen-journey-mock
ADMIN_PW=$(openssl rand -hex 12); ORG_PW=$(openssl rand -hex 6); MEMBER_PW=$(openssl rand -hex 6)
docker rm -f -v $MOCK $GW 2>/dev/null
docker run -d --name $MOCK -v "$PWD/scripts/mock_openai.py:/mock_openai.py:ro" \
  python:3.13-alpine python3 /mock_openai.py 8098 >/dev/null
docker run -d --name $GW -p 127.0.0.1:$PORT:8080 \
  -e TRIBUTARY_MASTER_KEY=$(openssl rand -hex 32) -e TRIBUTARY_ADMIN_PASSWORD="$ADMIN_PW" \
  $TAG >/dev/null
for i in $(seq 1 60); do
  [ "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:$PORT/healthz)" = 200 ] && break; sleep 1
done
curl -s -o /dev/null -w 'gateway %{http_code}\n' http://127.0.0.1:$PORT/healthz   # expect: gateway 200
docker exec $GW /usr/local/bin/entrypoint.sh tributary create-admin --email host@journey.test
```

expect: `gateway 200`, then the admin-created line. Anything else — read the
error `docker run` printed above it (a missing image, a busy port) before
going on.

> **No new image yet?** Preview THIS checkout's build instead — it is how
> the live check was done. First `pnpm -C apps/dashboard build:export`.
> Then, **before pasting B1**, run `TAG=tributary:m9exp` (any recent tag
> works), and in B1's gateway `docker run` line add
> `-v "$PWD/apps/dashboard/out:/usr/share/tributary/dashboard:ro"` right
> before `$TAG`.

### B2 — provision through the public API

Helpers first (`jval` / `jnum` pull one field out of JSON):

```bash
API=http://127.0.0.1:$PORT/api/v1
post() { curl -s -X POST "$API$1" -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' -d "$2"; }
get()  { curl -s "$API$1" -H "Authorization: Bearer $TOKEN"; }
jval() { grep -o "\"$1\":\"[^\"]*\"" | head -n1 | cut -d'"' -f4; }
jnum() { grep -o "\"$1\":[0-9]*" | head -n1 | cut -d: -f2; }

TOKEN=$(curl -s -X POST $API/auth/login -H 'Content-Type: application/json' \
  -d "{\"email\":\"host@journey.test\",\"password\":\"$ADMIN_PW\"}" | jval token)
ORG=$(post /orgs '{"name":"Journey","slug":"journey-1"}' | jval id)
POOL=$(post /pools "{\"org_id\":\"$ORG\",\"name\":\"journey-root\",\"kind\":\"root\",\"model_allowlist\":[\"m-test\"]}" | jval id)
MOCK_IP=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' $MOCK)
DEP=$(post /pools/$POOL/deposits "{\"provider\":\"openai\",\"key\":\"sk-demo-upstream-key\",\"base_url\":\"http://$MOCK_IP:8098/v1\",\"model_families\":[\"m-test\"],\"contributed_micros\":5000000}" | jval depositor_id)
post /members "{\"org_id\":\"$ORG\",\"email\":\"admin@journey.test\",\"password\":\"$ORG_PW\",\"role\":\"org_admin\"}" >/dev/null
MEMBER=$(post /members "{\"org_id\":\"$ORG\",\"email\":\"member@journey.test\",\"password\":\"$MEMBER_PW\"}" | jval id)
KEY=$(post /pools/$POOL/keys "{\"user_id\":\"$MEMBER\"}" | jval raw)
echo "pool=$POOL depositor=$DEP"     # expect: two 32-hex ids
```

Then two requests through the pool (they burn 2,700 micro-USD each — the
mock reports 11 prompt + 7 completion tokens):

```bash
for i in 1 2; do curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:$PORT/v1/chat/completions \
  -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -d '{"model":"m-test","messages":[{"role":"user","content":"hi"}]}'; done
sleep 1; get /pools/$POOL/bucket-positions
```

expect: `200` on each of two lines, then exactly one position:

```json
[{"depositor_id":"<DEP>","provider":"openai","model_family":"m-test","contributed_micros":5000000,"burned_micros":5400,"earned_micros":0}]
```

### B3–B7 — the screen

Open `http://127.0.0.1:8147/login/` and sign in as `admin@journey.test`
(print the password with `echo $ORG_PW`).

| # | Do | Expect | PASS when |
|---|----|--------|-----------|
| B3 | Sidebar → **Settlement** | Picker *journey-root*; one row `openai` · `m-test` · Contributed **$5.00** · Burned **$0.0054** · Earned **$0.00**; footer *Total · 1 position* · **$5.00** · **$0.0054** · **$0.00** | All six figures |
| B4 | Hover **$0.0054**, then **$5.00** | Tooltips `5,400 micro-USD` and `5,000,000 micro-USD` | The exact integers — never a rounded figure |
| B5 | Click **Settle last hour (8%)** | *Settled <time> → <one hour later>*; a `from_ms … · to_ms … · fee_bps 800` line; **Distributable $0.0054 · Fee (8%) $0.000432 · Paid out $0.004968 · Undistributed $0.00**; Payouts: one row (your depositor) **$0.004968**, footer *Paid out $0.004968* | 5,400 = 432 + 4,968 + 0, to the micro-unit |
| B6 | Look back at the table — do not refresh | Earned is now **$0.004968**; footer Earned **$0.004968** | Earned grew by exactly the payout; Contributed and Burned unchanged |
| B7 | Click **Settle** again | A NEW window (later `from_ms`); four **$0.00**; *No payouts — every burn in this pool was already settled.* | Nothing is paid twice |

### B8 — replay the run by hand

Put the two numbers from the screen's `from_ms … · to_ms …` line into the
call (this is step A8's curl):

```bash
FROM_MS=<from the screen>; TO_MS=<to the screen>
curl -s -X POST $API/pools/$POOL/settle -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"from_ms\":$FROM_MS,\"to_ms\":$TO_MS,\"fee_bps\":800}"
```

expect — **exactly the first run's answer**, with your depositor id:

```json
{"distributable_micros":5400,"fee_micros":432,"undistributed_micros":0,"payouts":[{"depositor_id":"<DEP>","provider":"openai","model_family":"m-test","amount_micros":4968}]}
```

### B9 — take it down (site restoration, workflow step 13)

```bash
docker rm -f -v $GW $MOCK
docker ps -a --format '{{.Names}}' | grep screen-journey || echo clean
```

expect: both names printed, then `clean`.

## What you are looking at

- **"Last hour" is the run's name, not a time filter.** Settle pays every
  burn not yet settled, whatever its age; the one-hour window is the
  run's identity. That is why an identical window replays instead of
  paying again — and why the screen says *"Pays every unsettled burn"*.
- **Earned is cumulative** — every payout ever credited. Burned never
  goes down.
- **Micro-USD, not USDC.** These are the off-chain books (SPEC D40); USDC
  is the on-chain payout token. The hover text says so on purpose.
- **A depositor with no burn earns nothing**, even having contributed the
  most — earnings follow the burn (D43). Trap 3 is one way to see that
  happen for the wrong reason.

## Report template

```
image tag / commit: ______________
A1 PASS/FAIL   A2 PASS/FAIL (link visible?)   A3 PASS/FAIL
A4 PASS/FAIL (burned: ______)   A5 PASS/FAIL (identity held? ____)
A6 PASS/FAIL (Σ earned = paid out? ____)   A7 PASS/FAIL   A8 PASS/FAIL (identical replay? ____)
B3 PASS/FAIL   B4 PASS/FAIL (tooltips exact? ____)   B5 PASS/FAIL
B6 PASS/FAIL   B7 PASS/FAIL (nothing paid twice? ____)   B8 PASS/FAIL
B9 teardown clean: YES/NO
Notes / anything that felt off: ________________________
```
