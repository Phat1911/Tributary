#!/usr/bin/env python3
"""J6 (M5): browser-level E2E runner — boots the REAL stack, serves the
dashboard's static export from the gateway binary itself (exactly what
the container does), then hands the browser to Playwright.

Stack: scripts/manual_journey_m4.py's mock upstream + gateway on SQLite,
plus TRIBUTARY_STATIC_DIR=<the dashboard's out/ export>. The spec
(apps/dashboard/e2e/) drives Chromium against it.

Usage (repo root):
  python3 scripts/e2e_j6.py

Requires (dev box / CI): the static export built (pnpm -C apps/dashboard
build:export), @playwright/test in node_modules, and Chromium (installs
under target/pw-browsers when PLAYWRIGHT_BROWSERS_PATH is set — this
runner sets it). Exit code 0 = journey passed.
"""

import os
import secrets
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import manual_journey_m4 as mj  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main() -> None:
    mj.GATEWAY = mj.GATEWAY_DEFAULT
    print("J6: dashboard E2E (Playwright, Chromium, full static export)")
    export = os.path.join(REPO, "apps", "dashboard", "out")
    if not os.path.isfile(os.path.join(export, "index.html")):
        sys.exit("no static export — run: pnpm -C apps/dashboard build:export")

    mock_url, _ = mj.start_mocks()
    # start_server spawns `serve` from os.environ; add the static layer
    # the same way the container's entrypoint does.
    os.environ["TRIBUTARY_STATIC_DIR"] = export
    proc = mj.start_server(mock_url)
    try:
        email = f"e2e-{secrets.token_hex(3)}@tributary.dev"
        password = "hunter2hunter2"
        mj._create_admin(email, password)
        r = subprocess.run(
            [
                "npx", "-y", "playwright@1.56.1", "test",
                "--config", os.path.join("apps", "dashboard", "playwright.config.ts"),
            ],
            cwd=REPO,
            env={
                **os.environ,
                "PLAYWRIGHT_BROWSERS_PATH": os.path.join(REPO, "target", "pw-browsers"),
                "E2E_BASE_URL": mj.GATEWAY,
                "E2E_ADMIN_EMAIL": email,
                "E2E_ADMIN_PASSWORD": password,
            },
        )
        sys.exit(r.returncode)
    finally:
        proc.terminate()
        proc.wait(timeout=10)


if __name__ == "__main__":
    main()
