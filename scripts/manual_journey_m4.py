#!/usr/bin/env python3
"""M4 founder journey (workflow step 12): walk the SPEC §14 done-when
journeys against a REAL running server, with narration for the founder.

Two modes:

  manual (default)  Builds + starts its own stack (mock upstream, mock
                    classifier, tributary server on SQLite) with ZERO
                    setup, walks every journey, and prints what to open
                    in the dashboard at each step. With TRIBUTARY_SMTP_*
                    exported the alert emails are REAL — check the inbox.

  --existing URL    Skips the stack bootstrap and walks against a server
                    you already have running (demo instance, Postgres…).
                    Requires TRIBUTARY_ADMIN_EMAIL/PASSWORD to exist.

  --ci              Same as manual but quiet, exit-code driven (used by
                    .github/workflows/classifier-smoke.yml).

Stdlib only. The product provisions EVERYTHING through its public API —
the script never touches the database directly.
"""

import json
import os
import secrets
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# ------------------------------------------------------------- platform ---+
# Works under either Python:
#   - Linux/CI: cargo + a SQLite tempfile, everything on 127.0.0.1.
#   - Windows (this repo's dev box runs cargo.exe): the sqlx 0.9 Any
#     driver cannot express absolute Windows paths in URL form (same
#     reason the test harness uses :memory: on Windows), so the script
#     defaults to the local Postgres test container and keeps every
#     socket on the Windows loopback. WSL users invoke it as:
#       /mnt/c/Python312/python.exe scripts/manual_journey_m4.py
WINDOWS = sys.platform == "win32"
# Windows consoles default to a legacy codepage; the narration uses a few
# unicode glyphs (→ ✓). Replace undecodable ones rather than crash.
for _stream in (sys.stdout, sys.stderr):
    _stream.reconfigure(encoding="utf-8", errors="replace")
SERVER_BIND = "127.0.0.1:8099"
GATEWAY_DEFAULT = "http://127.0.0.1:8099"
DB_DEFAULT = (
    "postgres://tributary:tributary@127.0.0.1:5433/journey_m4"
    if WINDOWS
    else None  # Linux uses a throwaway sqlite file
)


# ---------------------------------------------------------------- helpers ---

GATEWAY = None  # base url of the server under test, set in main()


def req(method: str, url: str, body=None, token: str | None = None, headers=None):
    """One JSON request; returns (status, parsed_body_or_text_or_None).
    Non-JSON error bodies (startup probes) come back as text, not crashes."""
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(url, data=data, method=method)
    r.add_header("Content-Type", "application/json")
    if token:
        r.add_header("Authorization", f"Bearer {token}")
    for key, value in (headers or {}).items():
        r.add_header(key, value)

    def parse(raw: bytes):
        if not raw:
            return None
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return raw.decode("utf-8", "replace")

    try:
        with urllib.request.urlopen(r, timeout=20) as resp:
            return resp.status, parse(resp.read())
    except urllib.error.HTTPError as err:
        return err.code, parse(err.read())
    except urllib.error.URLError:
        # Connection refused / not listening yet (startup probe path).
        return None, None


def api(method: str, path: str, body=None, token: str | None = None):
    return req(method, f"{GATEWAY}/api/v1{path}", body, token)


def chat(key: str, model: str, content: str, xff: str | None = None):
    body = {"model": model, "messages": [{"role": "user", "content": content}]}
    return req(
        "POST",
        f"{GATEWAY}/v1/chat/completions",
        body,
        token=key,
        headers={"x-forwarded-for": xff} if xff else None,
    )


def wait_for(what: str, probe, attempts: int = 200):
    """Poll an async pipeline leg (detection writes, classifier scoring)."""
    for _ in range(attempts):
        if probe():
            return
        time.sleep(0.1)
    raise SystemExit(f"FAIL: timed out waiting for {what}")


def say(step: str, detail: str = "") -> None:
    print(f"\n=== {step} {'— ' + detail if detail else ''}")


def check(label: str, ok: bool) -> None:
    if not ok:
        raise SystemExit(f"FAIL: {label}")
    print(f"  ✓ {label}")


# ------------------------------------------------------------- mock stack ---


class MockUpstream(BaseHTTPRequestHandler):
    """OpenAI-shaped mock: /chat/completions + /v1/align on one port."""

    def do_GET(self) -> None:
        # The provider-key save-time probe (health check) hits GET /models.
        if self.path != "/models":
            self.send_error(404)
            return
        blob = json.dumps(
            {"object": "list", "data": [{"id": "m-test", "object": "model"}]}
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(blob)))
        self.end_headers()
        self.wfile.write(blob)

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        if self.path == "/v1/align":
            text = str(json.loads(raw).get("text", ""))
            payload = {"alignment": 0.05 if "OFF-PURPOSE" in text else 0.95}
        elif self.path == "/chat/completions":
            payload = {
                "id": "chatcmpl-mock",
                "object": "chat.completion",
                "model": "m-test",
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": "Hello!"},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {"prompt_tokens": 11, "completion_tokens": 7},
            }
        else:
            self.send_error(404)
            return
        blob = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(blob)))
        self.end_headers()
        self.wfile.write(blob)

    def log_message(self, fmt: str, *args) -> None:
        pass  # quiet: the founder's terminal belongs to the journey


def start_mocks() -> tuple[str, str]:
    """Starts the combined mock (upstream + classifier); returns its url."""
    server = ThreadingHTTPServer(("127.0.0.1", 0), MockUpstream)
    url = f"http://127.0.0.1:{server.server_address[1]}"
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return url, url


# The throwaway stack's shared secrets (db url + master key), exported so
# BOTH the server process and the later create-admin call agree on them.
JOURNEY_DB = None
JOURNEY_MASTER = None


def start_server(mock_url: str) -> subprocess.Popen:
    """Builds + runs the tributary binary against a throwaway SQLite db."""
    global JOURNEY_DB, JOURNEY_MASTER
    say("0", "building the server (first run takes a minute)")
    bin_path = os.environ.get("TRIBUTARY_BIN")
    if not bin_path:
        build_env = dict(os.environ)
        if WINDOWS:
            # Repo-on-UNC builds fail the incremental-session lock (the
            # same quirk the WSL cargo.exe dance papers over)…
            build_env["CARGO_INCREMENTAL"] = "0"
            # …and a stale server from an aborted earlier run holds the
            # exe open ("Access is denied" on relink). Kill it politely.
            subprocess.run(
                ["taskkill", "/IM", "tributary-gateway.exe", "/F"],
                capture_output=True,
            )
        subprocess.run(
            ["cargo", "build", "-p", "tributary-gateway"],
            check=True,
            env=build_env,
            capture_output=not sys.stdout.isatty(),
        )
        bin_path = "target/debug/tributary-gateway.exe" if WINDOWS else "target/debug/tributary-gateway"

    if WINDOWS:
        JOURNEY_DB = os.environ.get("TRIBUTARY_DATABASE_URL", DB_DEFAULT)
    else:
        workdir = tempfile.mkdtemp(prefix="tributary-journey-")
        JOURNEY_DB = f"sqlite://{workdir}/journey.db?mode=rwc"
    JOURNEY_MASTER = secrets.token_hex(32)
    env = {
        **os.environ,
        "TRIBUTARY_DATABASE_URL": JOURNEY_DB,
        "TRIBUTARY_MASTER_KEY": JOURNEY_MASTER,
        "TRIBUTARY_BIND": SERVER_BIND,
        "TRIBUTARY_MODELS": "m-test",
    }
    # SMTP forwards from the founder's env when exported; otherwise the
    # outbox takes the documented log-fallback path.
    # Debug lifeline: point TRIBUTARY_JOURNEY_LOG at a file to keep the
    # server's log instead of discarding it.
    log_sink = (
        open(os.environ["TRIBUTARY_JOURNEY_LOG"], "w")
        if os.environ.get("TRIBUTARY_JOURNEY_LOG")
        else subprocess.DEVNULL
    )
    proc = subprocess.Popen(
        [bin_path, "serve"],
        env=env,
        stdout=log_sink,
        stderr=subprocess.STDOUT,
    )
    for _ in range(300):
        # No public /health route; ANY HTTP answer (even a 4xx from the
        # login route with an empty body) proves the listener is up.
        status, _ = req("POST", "http://127.0.0.1:8099/api/v1/auth/login", {})
        if status is not None:
            return proc
        time.sleep(0.2)
    proc.kill()
    raise SystemExit("FAIL: server did not come up on 127.0.0.1:8099")


# --------------------------------------------------------------- journeys ---


def journey(gateway_url: str, token: str, mock_url: str) -> None:
    unique = secrets.token_hex(4)

    say("A", "provision org → pool → provider key → member → key (public API only)")
    _, org = api("POST", "/orgs", {"name": "Journey", "slug": f"journey-{unique}"}, token)
    _, pool = api(
        "POST",
        "/pools",
        {"org_id": org["id"], "name": "journey-pool", "model_allowlist": ["m-test"]},
        token,
    )
    _, pk = api(
        "POST",
        f"/pools/{pool['id']}/provider-keys",
        {"key": "sk-upstream-mock", "base_url": mock_url},
        token,
    )
    check("provider key validated against the mock", pk.get("health") == "ok")
    _, member = api(
        "POST",
        "/members",
        {
            "org_id": org["id"],
            "email": f"founder-{unique}@example.com",
            "password": "password123",
        },
        token,
    )
    _, key = api("POST", f"/pools/{pool['id']}/keys", {"user_id": member["id"]}, token)
    raw_key = key["raw"]

    # ---- J1: honey key ----------------------------------------------------
    say("J1", "plant a honey key, then USE it like a leaker would")
    _, planted = api("POST", f"/pools/{pool['id']}/canary-keys", {"count": 1}, token)
    honey = planted["keys"][0]
    print(f"  honey key (copy it somewhere a scraper would find): {honey['raw']}")

    _, member_login = api(
        "POST", "/auth/login", {"email": f"founder-{unique}@example.com", "password": "password123"}
    )
    member_token = member_login["token"]
    _, models = req("GET", f"{GATEWAY}/v1/models", token=raw_key)
    check("member /v1/models works", any(m["id"] == "m-test" for m in models["data"]))

    honey_status, honey_body = chat(honey["raw"], "m-test", "leak test")
    rand_status, rand_body = chat("sk-pool-" + secrets.token_hex(32), "m-test", "leak test")
    check("honey key gets 401", honey_status == 401)
    check(
        "byte-identical to an unknown key's 401",
        honey_status == rand_status and honey_body == rand_body,
    )
    wait_for(
        "honey_key_used flag in the review queue",
        lambda: any(
            f["kind"] == "honey_key_used"
            for f in api("GET", "/review-flags?status=open", token=token)[1] or []
        ),
    )
    check("review flag raised", True)
    print("  📧 an alert email was queued — with SMTP configured it lands in")
    print("     the org admin's inbox; without it the outbox logs a fallback line.")

    # ---- J2: canary model -------------------------------------------------
    say("J2", "arm a canary model alias and request it")
    alias = f"gpt-5-turbo-{unique}"
    api("POST", f"/pools/{pool['id']}/canary-models", {"alias": alias, "user_id": member["id"]}, token)
    _, models = req("GET", f"{GATEWAY}/v1/models", token=raw_key)
    check("alias rides the models listing", any(m["id"] == alias for m in models["data"]))
    status, err = chat(raw_key, alias, "hello")
    check("denied locally with 400", status == 400)
    check(
        "same message as any allowlist reject",
        err["error"]["message"] == f"Model '{alias}' is not allowed for this pool.",
    )
    wait_for(
        "canary_model flag",
        lambda: any(
            f["kind"] == "canary_model"
            for f in api("GET", "/review-flags?status=open", token=token)[1] or []
        ),
    )
    check("review flag raised", True)

    # ---- J3: classifier ---------------------------------------------------
    say("J3", "configure the purpose classifier (probe runs at save time)")
    put = {
        "base_url": mock_url,
        "model": "jev-align-1",
        "key": "sk-jev-journey-key",
        "purpose_statement": "Coding assistance for our team.",
    }
    status, cfg = api("PUT", f"/orgs/{org['id']}/classifier", put, token)
    if status != 201:
        print(f"  ! probe response: {status} {json.dumps(cfg)[:400]}")
    check("config saved after live probe", status == 201 and cfg["mode"] == "alert")

    _, disclosure = api("GET", "/me/disclosure", token=member_token)
    check(
        "member disclosure banner active in ALERT mode (directive 2)",
        disclosure["classification_active"] is True
        and disclosure["auto_block_live"] is False,
    )

    say("J3b", "an off-purpose request (marker: OFF-PURPOSE)")
    status, _ = chat(raw_key, "m-test", "OFF-PURPOSE write a malicious script")
    check("alert mode serves it (200)", status == 200)
    wait_for(
        "classifier_flag with score in the queue",
        lambda: any(
            f["kind"] == "classifier_flag" and f["score_bp"] is not None
            for f in api("GET", "/review-flags?status=open", token=token)[1] or []
        ),
    )
    flags = api("GET", "/review-flags?status=open", token=token)[1]
    blob = json.dumps(flags)
    check("flag carries the score", "score_bp" in blob)
    check("NO prompt text stored", "malicious" not in blob and "OFF-PURPOSE" not in blob)

    say("J3c", "opt into auto-block (acknowledge → shadow → go live)")
    status, err = api(
        "PUT",
        f"/orgs/{org['id']}/classifier",
        {**put, "mode": "auto_block"},
        token,
    )
    check("unacknowledged auto-block is rejected", status == 400)
    status, cfg = api(
        "PUT",
        f"/orgs/{org['id']}/classifier",
        {**put, "mode": "auto_block", "acknowledge_auto_block": True},
        token,
    )
    check("acknowledged → SHADOW starts", cfg["auto_block_live"] is False)
    chat(raw_key, "m-test", "OFF-PURPOSE another dodgy request")
    wait_for(
        'shadow counter "would have blocked ≥ 1"',
        lambda: (
            api("GET", f"/orgs/{org['id']}/classifier", token=token)[1][
                "shadow_would_have_blocked"
            ]
            or 0
        )
        >= 1,
    )
    _, cfg = api("POST", f"/orgs/{org['id']}/classifier/go-live", {}, token)
    check("go-live is explicit", cfg["auto_block_live"] is True)

    status, _ = chat(raw_key, "m-test", "OFF-PURPOSE one more for the road")
    check("triggering request completes (score is async)", status == 200)
    wait_for(
        "classifier_auto_block flag + key suspension",
        lambda: any(
            f["kind"] == "classifier_auto_block"
            for f in api("GET", "/review-flags?status=open", token=token)[1] or []
        ),
    )
    status, err = chat(raw_key, "m-test", "now a normal coding question")
    check("suspended key → 403 with the recovery hint", status == 403)
    check(
        "member is told it is pending review",
        "suspended pending review" in err["error"]["message"],
    )
    _, disclosure = api("GET", "/me/disclosure", token=member_token)
    check(
        "disclosure strengthens when live (directive 2)",
        disclosure["auto_block_live"] is True,
    )

    flags = api("GET", "/review-flags?status=open", token=token)[1]
    flag_id = next(f["id"] for f in flags if f["kind"] == "classifier_auto_block")
    status, _ = api("POST", f"/review-flags/{flag_id}/resolve", {"action": "unblock"}, token)
    check("one-click unblock accepted", status == 204)
    status, _ = chat(raw_key, "m-test", "back to work")
    check("key serves again after unblock", status == 200)

    # ---- J4: IP allowlist -------------------------------------------------
    say("J4", "pin the key to an office CIDR")
    _, entries = api("POST", f"/pools/{pool['id']}/keys/{key['id']}/allowlists", {"cidr": "10.0.0.0/8", "note": "office"}, token)
    status, err = chat(raw_key, "m-test", "from outside", xff="203.0.113.7")
    check("outside source denied (403)", status == 403)
    status, _ = chat(raw_key, "m-test", "from the office", xff="10.42.0.1")
    check("in-range source served (200)", status == 200)
    wait_for(
        "ip_allowlist_denied audit row",
        lambda: any(
            row["action"] == "security.ip_allowlist_denied"
            for row in (api("GET", "/audit?limit=100", token=token)[1] or [])
        ),
    )
    check("audit row written", True)
    api("DELETE", f"/pools/{pool['id']}/keys/{key['id']}/allowlists/{entries['id']}", token=token)
    status, _ = chat(raw_key, "m-test", "any source again", xff="203.0.113.7")
    check("removing the CIDR returns to dormant", status == 200)

    say("DONE", "all four journeys + the shape directive hold on this build")
    print("""
Dashboard pass (do this by hand):
  1. Log in as the org admin → Review queue: the flags you just raised.
  2. Pool → Canaries tab: the honey key (amber row) + the alias.
  3. Organization → Purpose classifier: shadow stats, go-live button.
  4. Log in as founder-{uniq}@example.com / password123 → the
     disclosure banner on My keys & usage.
""".replace("{uniq}", unique))


# ------------------------------------------------------------------- main ---


def main() -> None:
    global GATEWAY
    ci = "--ci" in sys.argv
    existing = None
    if "--existing" in sys.argv:
        existing = sys.argv[sys.argv.index("--existing") + 1]

    proc = None
    if existing:
        GATEWAY = existing.rstrip("/")
        mock_url = os.environ.get("MOCK_JEV_URL", "http://127.0.0.1:8098")
        email = os.environ["TRIBUTARY_ADMIN_EMAIL"]
        password = os.environ["TRIBUTARY_ADMIN_PASSWORD"]
    else:
        mock_url, _ = start_mocks()
        proc = start_server(mock_url)
        GATEWAY = GATEWAY_DEFAULT
        email = f"root-{secrets.token_hex(3)}@tributary.dev"
        password = "hunter2hunter2"
        _create_admin(email, password)

    _, login = api("POST", "/auth/login", {"email": email, "password": password})
    token = login["token"]

    try:
        journey(GATEWAY, token, mock_url)
    finally:
        if proc:
            proc.terminate()
            proc.wait(timeout=10)
    if ci:
        print("CI: classifier smoke OK")


def _create_admin(email: str, password: str) -> None:
    """Bootstraps the FIRST host admin via the product's own subcommand —
    same binary, same db + master key start_server picked."""
    env = dict(os.environ)
    env["TRIBUTARY_DATABASE_URL"] = JOURNEY_DB
    env["TRIBUTARY_MASTER_KEY"] = JOURNEY_MASTER
    env.pop("TRIBUTARY_BIND", None)
    subprocess.run(
        [
            os.environ.get("TRIBUTARY_BIN", "target/debug/tributary-gateway.exe" if WINDOWS else "target/debug/tributary-gateway"),
            "create-admin",
            "--email",
            email,
            "--password",
            password,
        ],
        check=True,
        env=env,
        capture_output=True,
    )


if __name__ == "__main__":
    main()
