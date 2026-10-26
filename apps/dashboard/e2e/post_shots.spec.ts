// Launch-post screenshots (M5.5 slice 1) — S0 org home, S3 pool
// workbench, S1 review queue + S2 audit log, driven through the same
// Playwright rig as J6: a REAL Chromium against the REAL staged stack
// (E2E_BASE_URL, default the compose-free docker container on
// http://localhost:8111). This is not fabrication: every pixel on screen
// is the shipped image serving the staged org.
//
// Rig choices (per the chore kickoff): dark mode (the product's only
// palette — :root IS dark), 1440×900 viewport, clean context (fresh
// browser context, no extensions), deviceScaleFactor 2 for the post-ready
// shots and 1 as the clipboard-ready size (a second REAL shot, never a
// resize). Both a viewport-framed and a full-page variant are captured
// per scene so the founder can pick the framing without any image
// editing. Reusable as-is for the M5.5 demo video.
//
// Env (set by scripts/post_shots.py):
//   E2E_BASE_URL / E2E_ADMIN_EMAIL / E2E_ADMIN_PASSWORD / POST_SHOTS_DIR
import { expect, test } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";

const OUT_DIR = process.env.POST_SHOTS_DIR ?? resolve("target/post-shots");
const EMAIL = process.env.E2E_ADMIN_EMAIL;
const PASSWORD = process.env.E2E_ADMIN_PASSWORD;

if (!EMAIL || !PASSWORD) {
  // Fail loudly, not silently: screenshots of the login error page would
  // look almost right and be completely wrong.
  throw new Error("E2E_ADMIN_EMAIL / E2E_ADMIN_PASSWORD are required");
}

// One pass at a given device scale: login → S0 org home → S3 pool
// workbench → S1 review queue → S2 audit.
async function shootScenes(browser: import("@playwright/test").Browser, scale: number) {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: scale,
    // The dashboard palette is dark by design; dark also keeps native
    // widgets (scrollbars, form corners) consistent with it.
    colorScheme: "dark",
  });
  const page = await context.newPage();

  await page.goto("/login");
  // The login form uses id-labeled inputs (same stable selectors as J6).
  await page.locator("#email").fill(EMAIL);
  await page.locator("#password").fill(PASSWORD);
  await page.getByRole("button", { name: /sign in/i }).click();
  // Org admins land on /org — the org console.
  await expect(page).toHaveURL(/\/org/);

  // ---- S0: org home — roster + pools ("the console") ------------------
  // Wait for real data, not skeletons: one member row + the pool link.
  await expect(page.getByText("casey@labs.example")).toBeVisible();
  await expect(page.getByRole("link", { name: "Team Pool" })).toBeVisible();
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${OUT_DIR}/S0-org-home@${scale}x.png` });
  await page.screenshot({
    path: `${OUT_DIR}/S0-org-home@${scale}x-full.png`,
    fullPage: true,
  });

  // ---- S3: pool workbench — member keys, masked, default "Keys" tab ---
  // Navigate by clicking the pool link (not a hardcoded id) so a re-staged
  // stack with different UUIDs still shoots.
  await page.getByRole("link", { name: "Team Pool" }).click();
  await expect(page).toHaveURL(/\/org\/pools/);
  // The keys table renders masked forms only — waiting for the prefix
  // proves real key rows are on screen (never a raw key).
  await expect(page.getByText("sk-pool-").first()).toBeVisible();
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${OUT_DIR}/S3-pool-workbench@${scale}x.png` });
  await page.screenshot({
    path: `${OUT_DIR}/S3-pool-workbench@${scale}x-full.png`,
    fullPage: true,
  });

  // ---- S1: review queue — BOTH story flags must be in frame ----------
  await page.goto("/org/review");
  // D32: casey's key seen from two genuine /24 networks.
  await expect(page.getByText("Key used from multiple networks")).toBeVisible();
  // Classifier live catch: devon's key suspended by auto-block.
  await expect(page.getByText("Auto-block (key suspended)")).toBeVisible();
  await page.waitForTimeout(500); // let data skeletons/toasts settle
  await page.screenshot({ path: `${OUT_DIR}/S1-review-queue@${scale}x.png` });
  await page.screenshot({
    path: `${OUT_DIR}/S1-review-queue@${scale}x-full.png`,
    fullPage: true,
  });

  // ---- S2: audit log — the three story rows must exist on page 1 -----
  await page.goto("/audit");
  await expect(page.getByText("security.quota_denied").first()).toBeVisible();
  await expect(page.getByText("pool_key.rotated").first()).toBeVisible();
  await expect(page.getByText("approval.executed").first()).toBeVisible();
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${OUT_DIR}/S2-audit-log@${scale}x.png` });
  await page.screenshot({
    path: `${OUT_DIR}/S2-audit-log@${scale}x-full.png`,
    fullPage: true,
  });

  await context.close();
}

test("S0/S3/S1/S2 launch-post shots (2x and 1x)", async ({ browser }) => {
  mkdirSync(OUT_DIR, { recursive: true });
  await shootScenes(browser, 2);
  await shootScenes(browser, 1);
  // Sanity line for the runner's log: everything landed where expected.
  console.log(`shots written to ${OUT_DIR}`);
});
