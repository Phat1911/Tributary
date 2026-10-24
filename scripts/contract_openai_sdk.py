#!/usr/bin/env python3
"""J7 (M5): OpenAI SDK contract test — the /v1 surface is a CONTRACT
(AGENTS "API contract"), so it is exercised with the OFFICIAL clients,
not just hand-rolled requests.

Boots the same throwaway stack as the M4 founder journey (mock upstream
+ real gateway binary on SQLite), provisions everything through the
PUBLIC API, then runs one chat completion + one models.list through each
of:

  - the official Python SDK (`pip install --target <dir> openai`)
  - the official JS SDK   (`npm install openai`)

Both legs assert the response SHAPE the SDKs de-serialize — if the
gateway drifts from the OpenAI wire format, the SDK raises here.

Usage (from the repo root):
  python3 scripts/contract_openai_sdk.py

Exit code 0 = contract holds. Stdlib + the two SDKs only.
"""

import json
import os
import secrets
import subprocess
import sys

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


# The two SDK legs are INLINE payload scripts (not subprocess python -c)
# so failures print with full tracebacks to this process's stderr.
PY_LEG = r"""
import os, sys
sys.path.insert(0, os.environ["SDK_PY"])
from openai import OpenAI  # the official client under test

client = OpenAI(base_url=os.environ["GW"] + "/v1", api_key=os.environ["MEMBER_KEY"])
r = client.chat.completions.create(
    model="m-test",
    messages=[{"role": "user", "content": "hello"}],
    max_tokens=8,
)
content = r.choices[0].message.content
reply = os.environ["CHAT_REPLY"]
assert content and reply.lower() in content.lower(), f"bad content: {content!r}"
assert r.model == "m-test", f"model echoed wrong: {r.model}"
models = [m.id for m in client.models.list().data]
assert "m-test" in models, f"m-test missing from models.list: {models}"
print("python-sdk-ok", repr(content))
"""

JS_LEG = r"""
const { OpenAI } = require(process.env.NODE_PATH_ROOT + "/node_modules/openai");
(async () => {
  const client = new OpenAI({
    baseURL: process.env.GW + "/v1",
    apiKey: process.env.MEMBER_KEY,
  });
  const r = await client.chat.completions.create({
    model: "m-test",
    messages: [{ role: "user", content: "hello" }],
    max_tokens: 8,
  });
  const content = r.choices[0].message.content;
  if (!content || !content.toLowerCase().includes(process.env.CHAT_REPLY.toLowerCase())) {
    throw new Error("bad content: " + JSON.stringify(content));
  }
  if (r.model !== "m-test") throw new Error("model echoed wrong: " + r.model);
  const models = [];
  for await (const m of client.models.list()) models.push(m.id);
  if (!models.includes("m-test")) throw new Error("m-test missing: " + models);
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
    print("J7: OpenAI SDK contract (official Python + JS clients)")
    MOCK_URL, _ = mj.start_mocks()
    proc = mj.start_server(MOCK_URL)
    try:
        email = f"root-{secrets.token_hex(3)}@tributary.dev"
        mj._create_admin(email, "hunter2hunter2")
        _, login = mj.api("POST", "/auth/login", {"email": email, "password": "hunter2hunter2"})
        TOKEN = login["token"]
        member_key = provision()

        env = {
            "GW": mj.GATEWAY,
            "MEMBER_KEY": member_key,
            "CHAT_REPLY": CHAT_REPLY,
        }
        # Python leg: the SDK is importable from target/sdk-py.
        run_leg("python-sdk", [sys.executable, "-c", PY_LEG],
                {**env, "SDK_PY": os.path.abspath(SDK_PY)})
        # Node leg: `openai` resolves from the repo root's node_modules.
        run_leg("js-sdk", ["node", "-e", JS_LEG],
                {**env, "NODE_PATH_ROOT": os.getcwd()})
        print("J7: contract holds on both SDKs")
    finally:
        proc.terminate()
        proc.wait(timeout=10)


if __name__ == "__main__":
    main()
