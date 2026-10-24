// J6 (M5): the dashboard journey, driven by a real browser (Playwright,
// Chromium). Everything the browser does goes through the SAME origin
// the founder ships: the gateway serves this dashboard's static export
// at "/" and /api/v1 lives beside it — no test-only backdoors.
//
// Credentials arrive via env (E2E_ADMIN_EMAIL / E2E_ADMIN_PASSWORD);
// scripts/e2e_j6.py provisions the host admin with the product's own
// subcommand before launching the browser.
import { expect, test } from "@playwright/test";

test("J6: login → create org → org visible; UI and API agree (no skew)", async ({
  page,
}) => {
  // ---- Login ------------------------------------------------------------
  await page.goto("/login");
  // The login form uses id-labeled inputs (Label htmlFor=…), so the
  // stable selectors are the input ids, not placeholders.
  await page.locator("#email").fill(process.env.E2E_ADMIN_EMAIL!);
  await page.locator("#password").fill(process.env.E2E_ADMIN_PASSWORD!);
  await page.getByRole("button", { name: /sign in/i }).click();

  // Host admins land on /host — the org console.
  await expect(page).toHaveURL(/\/host/);

  // ---- Create an org through the UI --------------------------------------
  const orgName = `E2E Org ${Date.now()}`;
  await page.getByRole("button", { name: /new org|create org/i }).first().click();
  await page.getByPlaceholder("Acme Labs").fill(orgName);
  // Slug derives automatically or is typed; use the slug field if present.
  const slug = page.getByPlaceholder("acme-labs");
  if (await slug.isVisible().catch(() => false)) {
    slug.fill(`e2e-${Date.now()}`);
  }
  await page.getByRole("button", { name: /^create$/i }).click();

  // The org card appears in the host console's list.
  await expect(page.getByText(orgName).first()).toBeVisible({ timeout: 10_000 });

  // ---- Skew leg: the UI list and the API must agree ----------------------
  // Same-origin request with the browser's session cookie: whatever the
  // dashboard renders for the org list must exist in /api/v1/orgs.
  // The session cookie is Secure (SPEC §7); Chromium grants the
  // localhost http:// exception, but Playwright's API request context
  // does not — so the API call is made BY THE PAGE (a same-origin
  // fetch inside the browser), which is the truer test anyway: the
  // same code path the dashboard's client uses.
  const orgs = await page.evaluate<{ id: string; name: string }[], unknown>(async () => {
    const r = await fetch("/api/v1/orgs");
    if (!r.ok) throw new Error(`GET /api/v1/orgs -> ${r.status}`);
    return (await r.json()) as { id: string; name: string }[];
  });
  const fromApi = orgs.find((o) => o.name === orgName);
  expect(fromApi, "org created via UI is present in the API list").toBeDefined();
});
