# Founder manual journey — M5.5 slice 1 (landing + Tailscale rung)

You are the stranger here. Run this top to bottom BEFORE slice-1
close-out; it is the workflow-step-12 journey proof for the landing
page and the deploy ladder. Expected time: 10–15 minutes (Part C
optional, +10 if you actually climb the Tailscale rung).

Report format at the bottom — send the filled list back, one PASS/FAIL
per line, plus anything that felt off even if it technically passed.

## Pre-flight

- The landing should already be live at
  **https://phat1911.github.io/Tributary/** (deployed from `main` by
  the Pages workflow). If the URL does not load, stop and report —
  everything else waits on this.

## Part A — the landing page (click every CTA)

Open the URL in a **clean browser profile** (no extensions, no login
state).

| # | Do | Expect | PASS when |
|---|----|--------|-----------|
| A1 | Paste the URL into a **Discord message draft or LinkedIn post draft** (do not post) | A preview card renders: title, description, and the review-queue screenshot image | The card shows an IMAGE, not just a gray box (wait ~30 s if the first paste is bare — scrapers cache) |
| A2 | Read the page top to bottom, once, slowly | Free/self-host posture everywhere | NO wording about pricing, tiers, "managed", support offers, or waitlists anywhere; anything that smells like a sales funnel gets reported |
| A3 | Click header **Code** | GitHub repo root | Lands on `Phat1911/Tributary` |
| A4 | Click header **Deploy guide** | deploy/README.md rendered | Opens the deploy guide |
| A5 | Click header **Audit** | AUDIT.md rendered | Opens the audit log |
| A6 | Click hero CTA **Self-host it free today** | README, quickstart section scrolled into view | The `#quickstart` anchor scrolls (not the repo top) |
| A7 | Click hero CTA **Read the code** | Repo root | Loads |
| A8 | Scroll to the quickstart; click **sharing ladder** | deploy/README.md at the "Sharing ladder" heading | Anchor lands on the ladder section |
| A9 | Click **AUDIT.md** and **private disclosure** links in "Come break it" | Both open | Both load the right files |
| A10 | Footer: click **SPEC** and **FSL-1.1-ALv2** | Both open | Both load |
| A11 | Narrow the window to phone width | Layout reflows | No horizontal scrollbar; screenshots still visible; text readable |
| A12 | Check all four screenshots load (hero + three features) | Real dashboard screenshots, masked keys only | No broken-image icons; NO raw (unmasked) `sk-pool-…` middle anywhere |

## Part B — the Tailscale rung (a stranger's read)

Open
[`deploy/README.md`](https://github.com/Phat1911/Tributary/blob/main/deploy/README.md)
at **"Sharing ladder"**. The test: you have never used Tailscale — is
the rung followable WITHOUT asking anyone?

| # | Read | PASS when |
|---|------|-----------|
| B1 | Rung 1 (localhost) and Rung 2 (LAN) | Each is 1–4 concrete steps; nothing references a section that does not exist |
| B2 | Rung 3 (Tailscale), steps 1–6 in order | You could genuinely go from "never installed it" to "member opens http://100.x.y.z:8111" by following only this text; every command is copy-pasteable; every link is real |
| B3 | The trust note inside rung 3 | It clearly says: leave `TRIBUTARY_TRUSTED_PROXIES` unset, and never add `100.64.0.0/10`; the reason (X-Forwarded-For spoofing) makes sense in one read |
| B4 | The doctrine line | It states founder tailnets are never a production dependency |
| B5 | Rung 4 (public VPS) | It points at the trusted-proxies section (the next heading) rather than duplicating it |

## Part C — optional: actually climb rung 3 (+10 min, recommended once)

If you have (or install) Tailscale on any two devices: follow rung 3
literally, step by step, and confirm the staged demo dashboard
(http://100.x.y.z:8111) loads from the second device with Tailscale
on. Report any step where the guide and reality diverged — that text
is the deliverable, your divergence list is the review.

## Report template

```
A1 .. A12: PASS/FAIL (one line each, note anything odd)
B1 .. B5:  PASS/FAIL
C:         done / skipped + notes
Overall:   SHIP / FIX FIRST
```
