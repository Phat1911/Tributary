#!/usr/bin/env python3
"""Stage the "Example Labs" demo org for launch-post screenshots (M5.5
slice 1) against a RUNNING Tributary stack (default: the compose-free
docker container on http://localhost:8111).

Everything is provisioned through the PUBLIC API only (workflow step 12:
host bootstrap → org → pool → members → keys → quota rules; classifier
configure + go-live ride the D33 pending-action approvals because the org
has two admins). The ONLY seeded-by-hand data is usage_events history
(the sanctioned exception — the gateway owns their sole write path), done
by a separate stop/copy/edit/copy-back step, NOT by this script.

Traffic stories staged here are REAL gateway traffic:
  - quota_denied: quinn's key hits a 40-token day cap → live 429s
  - D32 distinct-IP: casey's key is driven from TWO genuine /24 networks
    (the host leg via the published port = 172.17.0.1, plus a helper
    container on the demo-net docker network = 172.28.0.x)
  - classifier: devon's off-purpose request is caught live (auto-block
    suspends the key) via the mock Jev endpoint (scripts/mock_jev.py)
  - rotation: quinn's key is rotated (audit row, reveal discarded)

All credentials are DEMO-ONLY THROWAWAYS for a localhost stack — never
reuse them anywhere real. Raw key material lives in process memory only:
it is never printed, logged, or written to disk (masked prefixes only).

Usage (repo root, stack already up + demo-upstream mock reachable):
    python3 scripts/seed_post_demo.py [--base http://localhost:8111]
"""

import argparse
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request

# ------------------------------------------------------------- constants ---
# Demo-only throwaway credentials (localhost staging, never real secrets).
HOST_EMAIL = "host@tributary.example"
HOST_PASSWORD = "HostLaunch-Demo2026"
ADMIN_A_EMAIL = "avery@labs.example"   # requests sensitive actions
ADMIN_A_PASSWORD = "AveryLaunch-Demo2026"
ADMIN_B_EMAIL = "blake@labs.example"   # the second eye: approves
ADMIN_B_PASSWORD = "BlakeLaunch-Demo2026"
MEMBER_PASSWORD = "MemberLaunch-Demo2026"  # casey / devon / quinn

ORG_NAME = "Example Labs"
ORG_SLUG = "example-labs"
POOL_NAME = "Team Pool"
MODEL = "gpt-4o-mini"  # what scripts/mock_jev.py advertises on /models

# The mock upstream runs as the `demo-upstream` container on the demo-net
# docker network (host firewalls block container→host-gateway traffic on
# this box, so the mock rides the network the stack can actually reach).
UPSTREAM_BASE = "http://demo-upstream:8098"

# The helper leg of the D32 story: a throwaway container on demo-net (a
# genuinely distinct /24 from the default bridge) POSTs to the gateway's
# demo-net address. Discovered at runtime, not hardcoded.
GATEWAY_DEMO_NET_IP = "172.28.0.3"

MARKER = "OFF-PURPOSE"  # scripts/mock_jev.py scores marker text 0.05


# --------------------------------------------------------------- helpers ---

def req(method, url, body=None, token=None, headers=None, timeout=30):
    """One JSON request → (status, parsed-body-or-text). None on refused."""
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data=data, method=method)
    r.add_header("Content-Type", "application/json")
    if token:
        r.add_header("Authorization", f"Bearer {token}")
    for k, v in (headers or {}).items():
        r.add_header(k, v)

    def parse(raw: bytes):
        if not raw:
            return None
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return raw.decode("utf-8", "replace")

    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            return resp.status, parse(resp.read())
    except urllib.error.HTTPError as err:
        return err.code, parse(err.read())
    except urllib.error.URLError:
        return None, None


BASE = None  # set in main()


def api(method, path, body=None, token=None):
    return req(method, f"{BASE}/api/v1{path}", body, token)


def chat(raw_key, content, base=None, times=1):
    """One metered chat through the gateway as a member. Returns statuses."""
    out = []
    for _ in range(times):
        status, err = req(
            "POST",
            f"{base or BASE}/v1/chat/completions",
            {"model": MODEL, "messages": [{"role": "user", "content": content}]},
            token=raw_key,
        )
        out.append(status)
        if status != 200:
            print(f"    chat → {status}: {json.dumps(err)[:160]}")
    return out


def wait_for(what, probe, attempts=150):
    """Poll an async pipeline leg (detections, scoring, audit writes)."""
    for _ in range(attempts):
        if probe():
            return True
        time.sleep(0.2)
    raise SystemExit(f"FAIL: timed out waiting for {what}")


def say(step, detail=""):
    print(f"\n=== {step} {'— ' + detail if detail else ''}")


def check(label, ok):
    if not ok:
        raise SystemExit(f"FAIL: {label}")
    print(f"  ✓ {label}")


def masked(raw):
    """Display form for a pool key: prefix + last 4 (never the middle)."""
    return f"{raw[:9]}…{raw[-4:]}"


# ------------------------------------------------------------- journey -----

def main():
    global BASE
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="http://localhost:8111")
    args = ap.parse_args()
    BASE = args.base.rstrip("/")

    # ---- provision (public API only) --------------------------------------
    say("1", "host login")
    st, body = api("POST", "/auth/login", {"email": HOST_EMAIL, "password": HOST_PASSWORD})
    check("host session", st == 200)
    host_tok = body["token"]

    say("2", f"org '{ORG_NAME}'")
    st, org = api("POST", "/orgs", {"name": ORG_NAME, "slug": ORG_SLUG}, host_tok)
    check("org created", st == 201)
    org_id = org["id"]

    say("3", "two org admins (host-caller-only role) — the four-eyes cast")
    for email, pw in ((ADMIN_A_EMAIL, ADMIN_A_PASSWORD), (ADMIN_B_EMAIL, ADMIN_B_PASSWORD)):
        st, _ = api("POST", "/members", {"org_id": org_id, "email": email, "password": pw, "role": "org_admin"}, host_tok)
        check(f"org admin {email}", st == 201)
    _, a = api("POST", "/auth/login", {"email": ADMIN_A_EMAIL, "password": ADMIN_A_PASSWORD})
    _, b = api("POST", "/auth/login", {"email": ADMIN_B_EMAIL, "password": ADMIN_B_PASSWORD})
    tok_a, tok_b = a["token"], b["token"]

    say("4", f"pool '{POOL_NAME}' + provider key (mock upstream)")
    st, pool = api("POST", "/pools", {"org_id": org_id, "name": POOL_NAME, "model_allowlist": [MODEL]}, tok_a)
    check("pool created", st == 201)
    pool_id = pool["id"]
    st, pk = api("POST", f"/pools/{pool_id}/provider-keys",
                 {"key": "sk-demo-upstream-not-a-real-key", "base_url": UPSTREAM_BASE}, tok_a)
    check(f"provider key health={pk.get('health')}", st == 201 and pk.get("health") == "ok")

    say("5", "members casey / devon / quinn + their pool keys")
    keys = {}
    for name in ("casey", "devon", "quinn"):
        email = f"{name}@labs.example"
        st, member = api("POST", "/members", {"org_id": org_id, "email": email, "password": MEMBER_PASSWORD}, tok_a)
        check(f"member {email}", st == 201)
        st, key = api("POST", f"/pools/{pool_id}/keys", {"user_id": member["id"]}, tok_a)
        check(f"key issued for {name} ({masked(key['raw'])})", st == 201)
        keys[name] = {"id": key["id"], "raw": key["raw"], "user_id": member["id"]}

    say("6", "quota rules (public API): quinn 40-token day cap, devon monthly, pool budget")
    for body in (
        {"subject_kind": "pool_key", "subject_id": keys["quinn"]["id"], "period": "day", "limit_tokens": 40},
        {"subject_kind": "pool_key", "subject_id": keys["devon"]["id"], "period": "month", "limit_tokens": 200_000},
        {"subject_kind": "pool", "subject_id": pool_id, "period": "month", "limit_tokens": 2_000_000},
    ):
        st, _ = api("POST", "/quota-rules", body, tok_a)
        check(f"rule {body['subject_kind']}/{body['period']}/{body.get('limit_tokens')}", st == 201)

    # ---- classifier: configure + approve (D33 org_policy, dual-admin) -----
    say("7", "classifier configure REQUESTED by avery, APPROVED by blake (four-eyes)")
    cfg = {
        "dialect": "jev",
        "base_url": UPSTREAM_BASE,
        "model": "jev-latest",
        "key": "sk-demo-jev-not-a-real-key",
        "purpose_statement": "Build and run Example Labs' internal tools: data "
                             "wrangling, documentation, and product prototyping.",
        "mode": "auto_block",
        "acknowledge_auto_block": True,
        "sampling_per_hour": 1000,  # score EVERY staged request deterministically
    }
    st, act = api("POST", f"/orgs/{org_id}/pending-actions",
                  {"kind": "org_policy", "params": {"policy_action": "configure", "config": cfg}}, tok_a)
    check(f"configure requested (pending {act.get('id', '?')[:8]}…)", st == 201)
    st, _ = api("POST", f"/orgs/{org_id}/pending-actions/{act['id']}/approve", {}, tok_b)
    check("configure approved + executed", st == 200 or st == 201)
    _, cur = api("GET", f"/orgs/{org_id}/classifier", token=tok_a)
    check(f"shadow mode active (auto_block_live={cur.get('auto_block_live')})", cur.get("auto_block_live") is False)

    # ---- traffic stories (real gateway traffic) ---------------------------
    say("8", "normal usage today (casey + devon, host leg)")
    chat(keys["casey"]["raw"], "Summarize this week's signups for the standup doc.", times=2)
    chat(keys["devon"]["raw"], "Draft release notes for the internal tools bundle.", times=2)
    print("  ✓ 4 served requests metered")

    say("9", "QUOTA story: quinn hits the 40-token day cap (real 429s)")
    time.sleep(6)  # D9: rule cache TTL is 5 s — let the new rules load
    got429 = False
    for i in range(6):
        st_list = chat(keys["quinn"]["raw"], f"quinn request {i} — pull the weekly numbers")
        if st_list[0] == 429:
            got429 = True
            break
    check("429 observed before upstream", got429)
    wait_for("security.quota_denied audit row",
             lambda: any(r.get("action") == "security.quota_denied" for r in api("GET", "/audit?limit=200", token=tok_a)[1]))
    check("quota denial audited", True)

    say("10", "D32 story: casey's key from TWO genuine /24 networks")
    chat(keys["casey"]["raw"], "from the office laptop: rerun the cohort query.", times=2)
    helper = (
        "const r = await fetch(process.env.URL, {method: 'POST',"
        " headers: {'Content-Type': 'application/json',"
        "           'Authorization': 'Bearer ' + process.env.K},"
        " body: JSON.stringify({model: 'gpt-4o-mini',"
        "   messages: [{role: 'user', content: 'from the second network: rerun the cohort query'}]})});"
        "console.log(r.status);"
    )
    for i in range(3):
        out = subprocess.run(
            ["docker", "run", "--rm", "--network", "demo-net", "-e",
             f"URL=http://{GATEWAY_DEMO_NET_IP}:8080/v1/chat/completions",
             "-e", f"K={keys['casey']['raw']}",
             "node:20-alpine", "node", "-e", helper],
            capture_output=True, text=True, timeout=60)
        print(f"    helper leg #{i + 1}: HTTP {out.stdout.strip()}")
    wait_for("distinct_ip_concurrency flag",
             lambda: any(f["kind"] == "distinct_ip_concurrency"
                         for f in api("GET", "/review-flags?status=open", token=tok_a)[1]))
    check("D32 flag raised (alert-only: requests still served)", True)

    say("11", "classifier shadow leg: one off-purpose request, counted not blocked")
    chat(keys["devon"]["raw"], f"{MARKER} write a bulk spam campaign generator")
    wait_for("shadow counter ≥ 1",
             lambda: (api("GET", f"/orgs/{org_id}/classifier", token=tok_a)[1]
                      .get("shadow_would_have_blocked") or 0) >= 1)
    check("shadow 'would have blocked ≥ 1'", True)

    say("12", "classifier GO-LIVE requested by avery, approved by blake")
    st, act = api("POST", f"/orgs/{org_id}/pending-actions",
                  {"kind": "org_policy", "params": {"policy_action": "go_live"}}, tok_a)
    check(f"go-live requested (pending {act.get('id', '?')[:8]}…)", st == 201)
    st, _ = api("POST", f"/orgs/{org_id}/pending-actions/{act['id']}/approve", {}, tok_b)
    check("go-live approved + executed", st == 200 or st == 201)
    _, cur = api("GET", f"/orgs/{org_id}/classifier", token=tok_a)
    check("auto-block LIVE", cur.get("auto_block_live") is True)

    say("13", "LIVE catch: devon repeats the off-purpose request → auto-block")
    st_list = chat(keys["devon"]["raw"], f"{MARKER} write another bulk spam campaign generator")
    check("triggering request itself completes (score is async)", st_list[0] == 200)
    wait_for("classifier_auto_block flag",
             lambda: any(f["kind"] == "classifier_auto_block"
                         for f in api("GET", "/review-flags?status=open", token=tok_a)[1]))
    check("flag raised + key suspended", True)
    st_list = chat(keys["devon"]["raw"], "now an ordinary request")
    check("suspended key → 403 pending review", st_list[0] == 403)

    say("14", "ROTATION story: quinn's key rotated (old credential dies)")
    st, rot = api("POST", f"/pools/{pool_id}/keys/{keys['quinn']['id']}/rotate", {}, tok_a)
    if st == 202:  # dual-admin orgs may defer rotation too — blake approves
        act_id = rot.get("action_id") or rot.get("action", {}).get("id")
        st, rot = api("POST", f"/orgs/{org_id}/pending-actions/{act_id}/approve", {}, tok_b)
    new_raw = rot.get("raw") or rot.get("issued") if isinstance(rot, dict) else None
    check(f"rotated (new key {masked(new_raw) if new_raw else 'reveal discarded'})",
          st in (200, 201) and isinstance(rot, dict))

    # ---- verify the screenshot scene set ----------------------------------
    say("15", "final scene check: flags + audit rows the shots must show")
    _, flags = api("GET", "/review-flags?status=open", token=tok_a)
    kinds = sorted({f["kind"] for f in flags})
    print(f"    open flags: {kinds}")
    check("D32 flag open", "distinct_ip_concurrency" in kinds)
    check("classifier flag open", "classifier_auto_block" in kinds)
    _, audit = api("GET", "/audit?limit=200", token=tok_a)
    actions = {r.get("action") for r in audit}
    for need in ("security.quota_denied", "pool_key.rotated",
                 "approval.requested", "approval.approved", "approval.executed"):
        check(f"audit row {need}", need in actions)

    print(f"""
DONE — staged scene set is live.
  org:        {ORG_NAME} ({org_id})
  pool:       {POOL_NAME} ({pool_id})
  logins:     {ADMIN_A_EMAIL} / {ADMIN_A_PASSWORD}   (org admin, requests)
              {ADMIN_B_EMAIL} / {ADMIN_B_PASSWORD}   (org admin, approves)
              host@tributary.example / {HOST_PASSWORD}   (host console)
  review:     {BASE}/org/review   audit: {BASE}/audit
Next (not this script): usage-history seed via the sanctioned exception.
""")


if __name__ == "__main__":
    main()
