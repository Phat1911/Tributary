#!/usr/bin/env python3
"""Mock purpose-classifier server (D28 "Jev" dialect) for manual journeys.

Speaks BOTH classifier dialects the gateway adapter can use:
- "custom": POST /v1/align, request {model, purpose, text} ->
  response {"alignment": <0..1>} (the generic scorer shape);
- "jev": POST /v1/systemone (TypeSafe AI's real hosted contract,
  director research 2026-10-21): request text rides in state.message,
  response score at answers.alignment.noul.

Also serves the minimal OpenAI-shaped upstream surface (GET /models,
POST /v1/chat/completions) so ONE window mocks both roles in a manual
stack: the pool's provider key and the org's classifier both point here
(the journey script's in-process mock plays the same combined role).

Scoring rule (same as the integration tests): the literal marker
"OFF-PURPOSE" anywhere in the text scores 0.05 (below both the alert
threshold 0.5 and the block threshold 0.15); anything else scores 0.95.

Usage:
    python3 scripts/mock_jev.py [port]        # default 8098

Stdlib only — no dependencies (repo rule: scripts stay runnable).
"""

import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer


class AlignHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        # Provider-key save-time probe hits GET {base_url}/models; answer
        # both spellings so the base URL may carry /v1 (the OpenAI
        # convention, and what the manual checklist recommends) or not.
        if self.path not in ("/models", "/v1/models"):
            self.send_error(404)
            return
        payload = json.dumps(
            {"object": "list", "data": [{"id": "gpt-4o-mini", "object": "model"}]}
        )
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload.encode())
    def do_POST(self) -> None:
        if self.path in ("/v1/chat/completions", "/chat/completions"):
            # Upstream role: the smallest legal completion response, with
            # the same usage numbers the integration mocks use (11+7=18).
            length = int(self.headers.get("Content-Length", "0"))
            self.rfile.read(length)
            payload = json.dumps(
                {
                    "id": "chatcmpl-mock",
                    "object": "chat.completion",
                    "model": "gpt-4o-mini",
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": "mock reply"},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {"prompt_tokens": 11, "completion_tokens": 7},
                }
            )
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload.encode())
            return
        if self.path == "/v1/systemone":
            # Real-dialect role (director research 2026-10-21): the TypeSafe
            # Jev contract. Score lives at answers.alignment.noul; the
            # request text rides in state.message; the purpose is embedded
            # in the noul question's instructions (we read it to show the
            # round-trip in the console log).
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length)
            try:
                body = json.loads(raw)
            except json.JSONDecodeError:
                self.send_error(400, "invalid JSON")
                return
            text = str(body.get("state", {}).get("message", ""))
            instructions = str(
                body.get("questions", {}).get("alignment", {}).get("instructions", "")
            )
            print(
                f"[mock] systemone model={body.get('model')} "
                f"instructions={instructions!r} message_len={len(text)}"
            )
            noul = 0.05 if "OFF-PURPOSE" in text else 0.95
            payload = json.dumps({"answers": {"alignment": {"noul": noul}}})
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload.encode())
            return
        if self.path != "/v1/align":
            self.send_error(
                404, "only /v1/systemone, /v1/align and /v1/chat/completions are implemented"
            )
            return
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        try:
            body = json.loads(raw)
        except json.JSONDecodeError:
            self.send_error(400, "invalid JSON")
            return
        text = str(body.get("text", ""))
        alignment = 0.05 if "OFF-PURPOSE" in text else 0.95
        payload = json.dumps({"alignment": alignment, "model": body.get("model")})
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload.encode())

    def log_message(self, fmt: str, *args) -> None:
        # Keep the founder's terminal readable; the marker line is enough.
        sys.stderr.write(f"[mock-jev] {self.address_string()} {fmt % args}\n")


def main() -> None:
    # argv: [port] [bind] — bind defaults to loopback; pass 0.0.0.0 when a
    # Docker container must reach the mock (staged demo stacks point the
    # container at the host's docker0 address, e.g. 172.17.0.1:8098).
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8098
    bind = sys.argv[2] if len(sys.argv) > 2 else "127.0.0.1"
    server = HTTPServer((bind, port), AlignHandler)
    print(f"mock classifier+upstream listening on http://{bind}:{port}")
    print('marker: include "OFF-PURPOSE" in a prompt to score 0.05')
    server.serve_forever()


if __name__ == "__main__":
    main()
