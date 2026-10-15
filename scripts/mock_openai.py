#!/usr/bin/env python3
"""Mock OpenAI upstream for local gateway smoke tests (never real OpenAI).

Serves POST /v1/chat/completions:
- stream=true  -> SSE chunks incl. usage chunk + [DONE]
- stream=false -> JSON completion with usage
Everything else -> 404. Requires no auth (asserts nothing); the gateway
forwards its own upstream key, which the gateway tests verify separately.
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

UPSTREAM_KEY = "sk-demo-upstream-key"

CHUNKS = [
    '{"id":"c1","object":"chat.completion.chunk","model":"m-test",'
    '"choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"}}]}',
    '{"id":"c1","object":"chat.completion.chunk","model":"m-test",'
    '"choices":[{"index":0,"delta":{"content":"lo"}}]}',
    '{"id":"c1","object":"chat.completion.chunk","model":"m-test",'
    '"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}',
    '{"id":"c1","object":"chat.completion.chunk","model":"m-test",'
    '"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":3}}',
]


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self.send_error(404)
            return
        length = int(self.headers.get("content-length", 0))
        body = json.loads(self.rfile.read(length) or b"{}")
        auth = self.headers.get("authorization", "")
        print(f"[mock] auth={auth} stream={body.get('stream')} "
              f"stream_options={body.get('stream_options')}", file=sys.stderr)
        if auth != f"Bearer {UPSTREAM_KEY}":
            self.send_error(401, "bad upstream key")
            return
        if body.get("stream"):
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.end_headers()
            for chunk in CHUNKS:
                self.wfile.write(f"data: {chunk}\n\n".encode())
            self.wfile.write(b"data: [DONE]\n\n")
        else:
            payload = json.dumps({
                "id": "chatcmpl-mock", "object": "chat.completion",
                "model": body.get("model", "m-test"),
                "choices": [{"index": 0,
                             "message": {"role": "assistant", "content": "Hello!"},
                             "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 11, "completion_tokens": 7},
            }).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    def log_message(self, *_args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9999
    print(f"mock openai on 0.0.0.0:{port} (upstream key: {UPSTREAM_KEY})",
          file=sys.stderr)
    ThreadingHTTPServer(("0.0.0.0", port), Handler).serve_forever()
