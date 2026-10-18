#!/usr/bin/env python3
"""Mock purpose-classifier server (D28 "Jev" dialect) for manual journeys.

Speaks the version-pinned POST /v1/align contract the gateway adapter
uses: request {model, purpose, text} -> response {"alignment": <0..1>}.

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
    def do_POST(self) -> None:
        if self.path != "/v1/align":
            self.send_error(404, "only /v1/align is implemented")
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
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8098
    server = HTTPServer(("127.0.0.1", port), AlignHandler)
    print(f"mock classifier listening on http://127.0.0.1:{port}/v1/align")
    print('marker: include "OFF-PURPOSE" in a prompt to score 0.05')
    server.serve_forever()


if __name__ == "__main__":
    main()
