import { defineConfig } from "@playwright/test";

// J6 (M5): browser-level E2E — workflow step 12 activates Playwright at
// M5. The STACK (mock upstream + real gateway serving this dashboard's
// static export) is booted by scripts/e2e_j6.py BEFORE `playwright test`
// runs; this config only points the browser at it. No webServer here —
// the stack is a Python-managed fixture (same bootstrap as the M4
// founder journey) so cargo/node/pnpm lifecycles stay out of the spec.
export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  // Serial on purpose: the journey is ONE story (login → org → pool) and
  // later steps depend on earlier ones; parallelism buys nothing here.
  workers: 1,
  retries: 0,
  use: {
    baseURL: process.env.E2E_BASE_URL ?? "http://127.0.0.1:8099",
    // Headless everywhere — CI has no display and the founder's script
    // must behave identically locally.
    headless: true,
  },
});
