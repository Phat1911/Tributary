# Founder manual journey — M5.5 slice 2 (Gemini provider, J-E)

You are the only person with a REAL Google AI Studio key — this journey
is the real-key leg (CI runs everything else against mocks; workflow
step 12: UI-visible change ⇒ a manual journey ships). Run it top to
bottom BEFORE slice-2 close-out. Expected time: **10–15 minutes**.

Report format at the bottom — send the filled list back, one PASS/FAIL
per line, plus anything that felt off even if it technically passed.

## Why this matters (read once)

The GDG DevFest entry (D36) honestly requires a Google technology —
this is it: a real Gemini key as a first-class pool provider. Demo Day
shows pooled Gemini with quotas and leak alarms. The journey also
settles the **model-ID question** for the demo: model names drift
(`2.0-flash` → `3.8-flash` in a year), so we verify at build time
against YOUR endpoint and record the answer — never hardcode blind.

## Pre-flight

- A running Tributary you administer (your own deployment —
  `http://localhost:8111`; the demo stack on :8112 works too). Login as
  host or org admin.
- A **Google AI Studio key**: <https://aistudio.google.com/apikey> →
  *Create API key* → copy it (starts `AIza…`). The free tier is enough.
- `curl` in a POSIX shell (WSL fine).

> **Rate-limit honesty, before you touch anything:** AI Studio's free
> tier is **rate-limited** (requests-per-minute AND per-day). A curl
> demo will not feel it; a whole team or an agent loop on one free key
> eventually will — the pool surfaces those upstream `429`s faithfully.
> Fine for demos and pilots. Knowing this NOW is the point: you must
> not meet this fact for the first time on stage Nov 14.

## Part J-E — provision → serve → govern, with your real key

| # | Do | Expect | PASS when |
|---|----|--------|-----------|
| E1 | `curl -s https://generativelanguage.googleapis.com/v1beta/openai/models -H "Authorization: Bearer AIza…"` | A JSON `data[]` list of models with `id`s | You can find the **flash** model's exact `id` (e.g. `gemini-3.8-flash` — trust the list, not this doc). **Write it down — every later step uses it.** Also note: the key went in a HEADER. The native Gemini API also accepts `?key=` URLs — never use that form; URLs leak into logs. |
| E2 | Dashboard → your org → **create pool**: name it `gemini-demo`, and in the model-allowlist field paste the model id from E1 | Pool created; allowlist shows the gemini id | The id appears in the pool's model list exactly as the endpoint reported it |
| E3 | Open the new pool → **Provider keys** tab → **Add provider key**: choose provider **Gemini (AI Studio key)**, paste your `AIza…` key, confirm base URL is `https://generativelanguage.googleapis.com/v1beta/openai`, save | Toast: *validated and sealed*; the key list shows the masked prefix (`AIzaS…xxxx`), provider **gemini**, health **ok** | The key shows health ok, the provider column says gemini, and the FULL key appears nowhere after the dialog closes (masked prefix only) |
| E4 | **Members**: pick or create a member, then **Issue pool key** for them on this pool; copy the raw member key (shown ONCE) | `sk-pool-…` key issued | You have the member key in your clipboard/notes |
| E5 | `curl -s http://localhost:8111/v1/chat/completions -H "Authorization: Bearer sk-pool-…" -H "Content-Type: application/json" -d '{"model":"<id-from-E1>","messages":[{"role":"user","content":"Say hello in Vietnamese, one sentence."}]}'` | A JSON chat completion whose `content` is an actual Gemini answer; `"model"` echoes the gemini id | A real answer came back THROUGH the pool — your AI-Studio key served a member key, metered |
| E6 | Pool page → **Usage** tab | A new usage row: the gemini model id, real token counts, your member | The row is visible with the gemini model name and nonzero tokens |
| E7 | Dashboard → the pool → **Quota rules**: add a `day` rule, limit `10` tokens, subject = this pool | Rule saved | Rule listed |
| E8 | Re-run the E5 curl with `"max_tokens": 8` added — twice | First call: 200. Second call: **429** naming the cap/limit — BEFORE any upstream call (no Google quota burned) | The 429 arrives with an explanatory message; a `security.quota_denied` audit row exists on the audit tab |
| E9 | Re-run E5 from a **different machine or phone hotspot** (same member key) within the window, then refresh the review-queue | Pool serves (sharing never blocks); a **distinct-IP concurrency** review flag fires for that member key | The flag lists the two /24 networks; the request itself still succeeded |

**After E1:** fill the *Verified on this deployment* slot in
[`deploy/README.md`](../deploy/README.md#provider-keys--openai-and-gemini)
(model id + date) — that is the doc's source of truth for the demo's
default example. Commit it.

## REAL-CONSUMER appendix — your own harness through the pool (AFTER the tick)

Not a done-when; J-E above stays curl. But the strongest Demo-Day
footage is your own AI harness drinking from the pool, so dogfood it
once the slice is ticked and **record the screen**:

1. In your harness's OpenAI settings, set
   `base_url = http://localhost:8111/v1` and
   `api_key = <the member sk-pool-… key>`. Model: the E1 id.
2. Run a real session. Every call is now metered, quota-checked, and
   IP-fingerprinted by your own gateway — usage rows should appear live
   on the pool page.
3. Expect the **free-tier rate limit** under sustained agent loops:
   upstream 429s surface honestly. Show one on camera — a pool that
   tells the truth about upstream limits is the product.
4. Save the recording: it is a demo-video scene (Demo Day, Nov 14; the
   video itself records ~Nov 12).

## Report template

```
E1  PASS/FAIL — flash model id seen: ______________
E2  PASS/FAIL
E3  PASS/FAIL (health: ____, provider column: ______)
E4  PASS/FAIL
E5  PASS/FAIL — answer snippet: _______________________
E6  PASS/FAIL (tokens: ____/____)
E7  PASS/FAIL
E8  PASS/FAIL (429 message ok? ____)
E9  PASS/FAIL (flag raised? ____)
deploy/README VERIFIED slot filled: YES/NO
Notes / anything that felt off: ________________________
```
