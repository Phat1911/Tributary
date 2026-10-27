#!/usr/bin/env python3
"""J7 (M5) + J-D (M5.5 slice 2): OpenAI SDK contract test — the /v1
surface is a CONTRACT (AGENTS "API contract"), so it is exercised with
the OFFICIAL clients, not just hand-rolled requests.

Boots the same throwaway stack as the M4 founder journey (mock upstream
+ real gateway binary on SQLite), provisions everything through the
PUBLIC API, then runs one chat completion + one models.list through each
of:

  - the official Python SDK (`pip install --target <dir> openai`)
  - the official JS SDK   (`npm install openai`)

M5.5 adds the J-D leg: a second pool routed to a MOCK Gemini-compat
upstream (the OpenAI-compatible endpoint shape Google serves at
generativelanguage.googleapis.com/v1beta/openai/). NO real key in CI —
the founder's real-key leg is the manual J-E journey. Both legs assert
the response SHAPE the SDKs de-serialize — if the gateway drifts from
the OpenAI wire format, or gemini-* models break anything the SDKs
touch, they raise here.

Usage (from the repo root):
  python3 scripts/contract_openai_sdk.py

Exit code 0 = contract holds. Stdlib + the two SDKs only.
"""

import json
import os
import secrets
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# Reuse the journey's stack bootstrap: importing (not copying) keeps the
# mock-upstream + server-spawn logic in ONE place — if it drifts, both
# scripts fail together instead of silently diverging.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import manual_journey_m4 as mj  # noqa: E402

# The Python SDK lives where this session installed it (kept OUT of the
# repo's runtime deps — this is a dev/CI-only tool). CI does the same
# `pip install --target` before invoking.
SDK_PY = os.path.join("target", "sdk-py")

# chat + models assertions shared by both SDK legs (same bar for both).
# The mock upstream answers a FIXED "Hello!" — the contract is
# about the WIRE SHAPE (SDK de-serialization), not the content.
CHAT_REPLY = "Hello"

# The Gemini-compat leg's fixtures. The model NAME mirrors the real
# endpoint's free-tier flash default; identities are config, never code.
GEMINI_MODEL = "gemini-3.8-flash"
GEMINI_KEY = "AIzaSyMOCK-gemini-key-0123456789abcdef"
GEMINI_REPLY = "Hello from gemini compat"


class GeminiCompatMock(BaseHTTPRequestHandler):
    """Mock of Google's OpenAI-COMPATIBLE endpoint: the same wire shape
    as the OpenAI mock, served under /v1beta/openai/ with a gemini-*
    model list. Two teaching pins live here:

    - Auth is `Authorization: Bearer` ONLY. The native Gemini API also
      accepts `?key=` query params — keys in URLs leak into logs, so any
      request arriving with a query string is REJECTED (401). If the
      gateway ever regressed to query-param auth, this leg goes red.
    - The health probe (GET {base}/models) and the serving path share
      the base URL exactly like the real endpoint.
    """

    def _bearer_ok(self) -> bool:
        auth = self.headers.get("Authorization", "")
        return auth == f"Bearer {GEMINI_KEY}"

    def do_GET(self) -> None:
        # Key-health probe + the models.list shape (J-E's enumeration
        # curl hits the same route on the real endpoint).
        if self.path != "/v1beta/openai/models":
            self.send_error(404)
            return
        if "?" in self.path or not self._bearer_ok():
            self.send_error(401)
            return
        blob = json.dumps(
            {
                "object": "list",
                "data": [
                    {"id": GEMINI_MODEL, "object": "model", "owned_by": "google"},
                    {"id": "gemini-3.8-pro", "object": "model", "owned_by": "google"},
                ],
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(blob)))
        self.end_headers()
        self.wfile.write(blob)

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        if self.path != "/v1beta/openai/chat/completions":
            self.send_error(404)
            return
        if "?" in self.path or not self._bearer_ok():
            self.send_error(401)
            return
        payload = {
            "id": "chatcmpl-gemini-mock",
            "object": "chat.completion",
            "model": GEMINI_MODEL,
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": GEMINI_REPLY + "!"},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 11, "completion_tokens": 7},
        }
        blob = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(blob)))
        self.end_headers()
        self.wfile.write(blob)

    def log_message(self, fmt: str, *args) -> None:
        pass  # quiet: matches the OpenAI mock's posture


def start_gemini_mock() -> str:
    """Starts the compat mock on an ephemeral port; returns its base URL
    (the path prefix included — that is what the pool stores)."""
    server = ThreadingHTTPServer(("127.0.0.1", 0), GeminiCompatMock)
    url = f"http://127.0.0.1:{server.server_address[1]}/v1beta/openai"
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return url


def provision():
    """org → pool → provider key → member → member key, public API only
    (workflow step 12: no direct DB seeding for provisionable things)."""
    unique = secrets.token_hex(4)
    email = f"sdk-{unique}@example.com"
    _, org = mj.api("POST", "/orgs", {"name": "SDK", "slug": f"sdk-{unique}"}, TOKEN)
    _, pool = mj.api(
        "POST",
        "/pools",
        {"org_id": org["id"], "name": "sdk-pool", "model_allowlist": ["m-test"]},
        TOKEN,
    )
    mj.api(
        "POST",
        f"/pools/{pool['id']}/provider-keys",
        {"key": "sk-upstream-mock", "base_url": MOCK_URL},
        TOKEN,
    )
    _, member = mj.api("POST", "/members", {"org_id": org["id"], "email": email,
                                            "password": "password123"}, TOKEN)
    _, key = mj.api("POST", f"/pools/{pool['id']}/keys", {"user_id": member["id"]}, TOKEN)
    return key["raw"]


def provision_gemini(base_url: str):
    """Same journey, GEMINI-labeled: pool with a gemini-* allowlist +
    provider key on the compat endpoint (J-A's public-API path)."""
    unique = secrets.token_hex(4)
    email = f"sdk-gem-{unique}@example.com"
    _, org = mj.api("POST", "/orgs", {"name": "SDK-Gemini", "slug": f"sg-{unique}"}, TOKEN)
    _, pool = mj.api(
        "POST",
        "/pools",
        {"org_id": org["id"], "name": "sdk-gemini-pool",
         "model_allowlist": [GEMINI_MODEL]},
        TOKEN,
    )
    mj.api(
        "POST",
        f"/pools/{pool['id']}/provider-keys",
        {"provider": "gemini", "key": GEMINI_KEY, "base_url": base_url},
        TOKEN,
    )
    _, member = mj.api("POST", "/members", {"org_id": org["id"], "email": email,
                                            "password": "password123"}, TOKEN)
    _, key = mj.api("POST", f"/pools/{pool['id']}/keys", {"user_id": member["id"]}, TOKEN)
    return key["raw"]


# The two SDK legs are INLINE payload scripts (not subprocess python -c)
# so failures print with full tracebacks to this process's stderr.
# MODEL arrives via env so the SAME leg code runs the OpenAI leg and the
# gemini-compat leg — one contract, two upstreams.
PY_LEG = r"""
import os, sys
sys.path.insert(0, os.environ["SDK_PY"])
from openai import OpenAI  # the official client under test

model = os.environ["LEG_MODEL"]
client = OpenAI(base_url=os.environ["GW"] + "/v1", api_key=os.environ["MEMBER_KEY"])
r = client.chat.completions.create(
    model=model,
    messages=[{"role": "user", "content": "hello"}],
    max_tokens=8,
)
content = r.choices[0].message.content
reply = os.environ["CHAT_REPLY"]
assert content and reply.lower() in content.lower(), f"bad content: {content!r}"
assert r.model == model, f"model echoed wrong: {r.model}"
models = [m.id for m in client.models.list().data]
assert model in models, f"{model} missing from models.list: {models}"
print("python-sdk-ok", repr(content))
"""

JS_LEG = r"""
const { OpenAI } = require(process.env.NODE_PATH_ROOT + "/node_modules/openai");
(async () => {
  const model = process.env.LEG_MODEL;
  const client = new OpenAI({
    baseURL: process.env.GW + "/v1",
    apiKey: process.env.MEMBER_KEY,
  });
  const r = await client.chat.completions.create({
    model: model,
    messages: [{ role: "user", content: "hello" }],
    max_tokens: 8,
  });
  const content = r.choices[0].message.content;
  if (!content || !content.toLowerCase().includes(process.env.CHAT_REPLY.toLowerCase())) {
    throw new Error("bad content: " + JSON.stringify(content));
  }
  if (r.model !== model) throw new Error("model echoed wrong: " + r.model);
  const models = [];
  for await (const m of client.models.list()) models.push(m.id);
  if (!models.includes(model)) throw new Error(model + " missing: " + models);
  console.log("js-sdk-ok", JSON.stringify(content));
})().catch((e) => { console.error(e); process.exit(1); });
"""


def run_leg(name: str, cmd: list[str], env: dict) -> None:
    """Runs one SDK leg; raises (with output) if it fails."""
    r = subprocess.run(cmd, env={**os.environ, **env}, capture_output=True, text=True)
    # Show the leg's output BEFORE check() — it exits on failure, and
    # the traceback is the whole point of the printout.
    print(f"  {name} stdout: {r.stdout.strip()[:400]}")
    if r.returncode != 0:
        print(f"  {name} stderr: {r.stderr.strip()[:1200]}")
    mj.check(f"{name}: chat.completions + models.list against /v1", r.returncode == 0)


def main() -> None:
    global MOCK_URL, TOKEN
    mj.GATEWAY = mj.GATEWAY_DEFAULT
    print("J7 + J-D: OpenAI SDK contract (official Python + JS clients, OpenAI + Gemini-compat upstreams)")
    MOCK_URL, _ = mj.start_mocks()
    GEMINI_URL = start_gemini_mock()
    proc = mj.start_server(MOCK_URL)
    try:
        email = f"root-{secrets.token_hex(3)}@tributary.dev"
        mj._create_admin(email, "hunter2hunter2")
        _, login = mj.api("POST", "/auth/login", {"email": email, "password": "hunter2hunter2"})
        TOKEN = login["token"]
        member_key = provision()
        gemini_key = provision_gemini(GEMINI_URL)

        def env_for(key: str, reply: str, model: str) -> dict:
            return {
                "GW": mj.GATEWAY,
                "MEMBER_KEY": key,
                "CHAT_REPLY": reply,
                "LEG_MODEL": model,
            }

        # OpenAI-routed legs (the J7 bar, unchanged).
        run_leg("python-sdk/openai", [sys.executable, "-c", PY_LEG],
                {**env_for(member_key, CHAT_REPLY, "m-test"),
                 "SDK_PY": os.path.abspath(SDK_PY)})
        run_leg("js-sdk/openai", ["node", "-e", JS_LEG],
                {**env_for(member_key, CHAT_REPLY, "m-test"),
                 "NODE_PATH_ROOT": os.getcwd()})
        # Gemini-compat legs (J-D): same SDKs, gemini-* model, compat
        # upstream — the contract must hold identically.
        run_leg("python-sdk/gemini", [sys.executable, "-c", PY_LEG],
                {**env_for(gemini_key, GEMINI_REPLY, GEMINI_MODEL),
                 "SDK_PY": os.path.abspath(SDK_PY)})
        run_leg("js-sdk/gemini", ["node", "-e", JS_LEG],
                {**env_for(gemini_key, GEMINI_REPLY, GEMINI_MODEL),
                 "NODE_PATH_ROOT": os.getcwd()})
        print("J7 + J-D: contract holds on both SDKs against both upstreams")
    finally:
        proc.terminate()
        proc.wait(timeout=10)


if __name__ == "__main__":
    main()
