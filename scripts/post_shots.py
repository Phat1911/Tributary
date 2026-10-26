#!/usr/bin/env python3
"""Take the S1/S2 launch-post screenshots (M5.5 slice 1) with the J6
Playwright rig — real Chromium against the REAL staged stack.

Boots nothing: it expects a running stack (default: the compose-free
docker container on http://localhost:8111, staged by
scripts/seed_post_demo.py) and drives apps/dashboard/e2e/post_shots.spec.ts
against it via the same npx-playwright invocation scripts/e2e_j6.py uses
(Chromium comes from the shared target/pw-browsers cache).

Output: PNGs into --out (default <repo>/target/post-shots, gitignored):
  S0-org-home@2x.png        (+ -full)  roster + pools ("the console")
  S3-pool-workbench@2x.png  (+ -full)  member keys (masked) + budgets
  S1-review-queue@2x.png (+ -full)   the review queue, both story flags
  S2-audit-log@2x.png     (+ -full)  quota_denied + rotation + approvals
  …plus @1x variants as clipboard-ready sizes (real shots, not resizes).

Credentials default to the seed script's demo throwaways (localhost demo
only — never reuse anywhere real); override with --email/--password.

Usage:  python3 scripts/post_shots.py [--base http://localhost:8111]
                                [--out DIR] [--email X --password Y]
"""

import argparse
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import seed_post_demo as seed  # demo credentials + repo layout live there

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="http://localhost:8111")
    ap.add_argument("--out", default=os.path.join(REPO, "target", "post-shots"))
    ap.add_argument("--email", default=seed.ADMIN_A_EMAIL)
    ap.add_argument("--password", default=seed.ADMIN_A_PASSWORD)
    args = ap.parse_args()

    env = dict(
        os.environ,
        # Same shared browser cache the J6 runner uses — no new downloads.
        PLAYWRIGHT_BROWSERS_PATH=os.path.join(REPO, "target", "pw-browsers"),
        E2E_BASE_URL=args.base.rstrip("/"),
        E2E_ADMIN_EMAIL=args.email,
        E2E_ADMIN_PASSWORD=args.password,
        POST_SHOTS_DIR=os.path.abspath(args.out),
    )
    print(f"shooting S1/S2 against {args.base} → {env['POST_SHOTS_DIR']}")
    # Filename filter `post_shots` keeps the J6 journey spec out of the run.
    cmd = ["npx", "-y", "playwright@1.56.1", "test", "post_shots",
           "--config", os.path.join("apps", "dashboard", "playwright.config.ts")]
    done = subprocess.run(cmd, cwd=REPO, env=env)
    sys.exit(done.returncode)


if __name__ == "__main__":
    main()
