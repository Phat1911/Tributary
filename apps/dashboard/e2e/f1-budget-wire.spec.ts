// M6-S1-F1 (fixed in v0.1.3): the Set-budget dialog must send the wire
// field that matches the window — minute ("requests") → limit_requests,
// day/month ("tokens") → limit_tokens. v0.1.2 shipped the minute window
// sending limit_tokens, silently turning "5 requests per minute" into a
// 5-TOKEN-per-minute cap that denied every request at reserve time (every
// request reserves prompt-chars/4 + 256 ≫ 5). This spec pins BOTH the
// wire payload (so the class cannot regress) and the enforcement meaning
// (5 requests pass, the next 429 names the REQUEST window).
//
// Env (set by scripts/e2e_j6.py): E2E_BASE_URL, E2E_ADMIN_EMAIL,
// E2E_ADMIN_PASSWORD, E2E_MOCK_URL — the in-process mock upstream. Its
// provider base URL is the BARE url (it serves /models and
// /chat/completions WITHOUT the /v1 prefix).
import { expect, test } from "@playwright/test";

const MEMBER_EMAIL = "f1-member@tributary.dev";

test("F1: minute budget sends limit_requests; 5 pass, the 6th 429 names the request window", async ({
  page,
}) => {
  test.setTimeout(120_000);

  // ---- Setup: host admin provisions the org + its admin (the product's
  // own escalation path), via the public API the dashboard itself uses.
  // The dialog under test is driven through the UI below — that payload
  // is the thing being pinned, so it must be produced by real clicks.
  await page.goto("/login");
  await page.locator("#email").fill(process.env.E2E_ADMIN_EMAIL!);
  await page.locator("#password").fill(process.env.E2E_ADMIN_PASSWORD!);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(page).toHaveURL(/\/host/, { timeout: 15_000 });

  const stamp = Date.now();
  const pilot = { email: `f1-${stamp}@tributary.dev`, pw: "f1-pilot-pass-9" };
  const { orgId } = await page.evaluate(
    async ({ stamp, pilot }) => {
      const post = async (path: string, body: unknown) => {
        const r = await fetch(`/api/v1${path}`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        });
        if (!r.ok)
          throw new Error(`POST ${path} -> ${r.status}: ${await r.text()}`);
        return r.json();
      };
      const org = await post("/orgs", {
        name: `F1 Org ${stamp}`,
        slug: `f1-${stamp}`,
      });
      await post("/members", {
        org_id: org.id,
        email: pilot.email,
        password: pilot.pw,
        role: "org_admin",
      });
      return { orgId: org.id as string };
    },
    { stamp, pilot },
  );

  // ---- Org admin provisions member + pool + provider key + member key.
  await page.context().clearCookies();
  await page.goto("/login");
  await page.locator("#email").fill(pilot.email);
  await page.locator("#password").fill(pilot.pw);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(page).toHaveURL(/\/org/, { timeout: 15_000 });

  const mockUrl = process.env.E2E_MOCK_URL!;
  const { poolId, rawKey } = await page.evaluate(
    async ({ orgId, mockUrl, stamp, memberEmail }) => {
      const post = async (path: string, body: unknown) => {
        const r = await fetch(`/api/v1${path}`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        });
        if (!r.ok)
          throw new Error(`POST ${path} -> ${r.status}: ${await r.text()}`);
        return r.json();
      };
      const member = await post("/members", {
        org_id: orgId,
        email: memberEmail,
        password: "f1-member-pw-9",
      });
      const pool = await post("/pools", {
        org_id: orgId,
        name: `f1-pool-${stamp}`,
        model_allowlist: ["m-test"],
      });
      // The mock serves /models + /chat/completions at its root; the
      // save-time probe validates the key against it.
      await post(`/pools/${pool.id}/provider-keys`, {
        provider: "custom",
        key: "sk-f1-mock-upstream",
        base_url: mockUrl,
      });
      const key = await post(`/pools/${pool.id}/keys`, {
        user_id: member.id,
      });
      return { poolId: pool.id as string, rawKey: key.raw as string };
    },
    { orgId, mockUrl, stamp, memberEmail: MEMBER_EMAIL },
  );

  // ---- The F1 surface: Set budget → One member → Minute (requests) → 5.
  await page.goto(`/org/pools/?id=${poolId}`);
  await page.getByRole("tab", { name: /budgets/i }).click();
  await page.getByRole("button", { name: /set budget/i }).click();
  await page.locator("#budget-scope").click();
  await page.getByRole("option", { name: /one member/i }).click();
  await page.locator("#budget-period").click();
  await page.getByRole("option", { name: /minute \(requests\)/i }).click();
  await page.locator("#budget-member").click();
  await page.getByRole("option", { name: MEMBER_EMAIL }).click();
  await page.locator("#budget-tokens").fill("5");
  await page.getByRole("button", { name: /save budget/i }).click();
  await expect(page.getByText(/minute/i).first()).toBeVisible({
    timeout: 10_000,
  });

  // ---- PIN 1: the wire payload. The minute-requests rule must carry
  // limit_requests (and NOT limit_tokens) — the exact v0.1.2 defect.
  const minuteRule = await page.evaluate(async () => {
    const rules = (await (await fetch("/api/v1/quota-rules")).json()) as {
      period: string;
      limit_tokens: number | null;
      limit_requests: number | null;
    }[];
    return rules.find((r) => r.period === "minute");
  });
  console.log(`F1_RULE_ON_WIRE=${JSON.stringify(minuteRule)}`);
  expect(minuteRule, "minute rule exists").toBeDefined();
  expect(minuteRule!.limit_requests, "minute window sends limit_requests").toBe(5);
  expect(minuteRule!.limit_tokens, "minute window must NOT send limit_tokens").toBeNull();

  // The table shows the honest meaning too — requests, not tokens.
  await expect(page.getByText("5 requests").first()).toBeVisible({
    timeout: 10_000,
  });

  // ---- PIN 1b: the inverse mapping — a Day (token) budget must keep
  // sending limit_tokens, NOT limit_requests. Pins the whole window→field
  // mapping so neither half can regress.
  await page.getByRole("button", { name: /set budget/i }).click();
  await page.locator("#budget-scope").click();
  await page.getByRole("option", { name: /one member/i }).click();
  await page.locator("#budget-period").click();
  await page.getByRole("option", { name: /^day/i }).click();
  await page.locator("#budget-member").click();
  await page.getByRole("option", { name: MEMBER_EMAIL }).click();
  await page.locator("#budget-tokens").fill("100000");
  await page.getByRole("button", { name: /save budget/i }).click();
  await expect(page.getByText("100,000 tokens").first()).toBeVisible({
    timeout: 10_000,
  });
  const dayRule = await page.evaluate(async () => {
    const rules = (await (await fetch("/api/v1/quota-rules")).json()) as {
      period: string;
      limit_tokens: number | null;
      limit_requests: number | null;
    }[];
    return rules.find((r) => r.period === "day");
  });
  console.log(`F1_DAY_ON_WIRE=${JSON.stringify(dayRule)}`);
  expect(dayRule, "day rule exists").toBeDefined();
  expect(dayRule!.limit_tokens, "day window sends limit_tokens").toBe(100000);
  expect(dayRule!.limit_requests, "day window must NOT send limit_requests").toBeNull();

  // ---- PIN 2: enforcement semantics. 5 requests pass; the next 429
  // names the REQUEST window (pre-fix, EVERY request denied naming a
  // 5-token window). Rare flake guard: if the burst straddled a minute
  // boundary (no 429 at all), realign to a fresh minute and re-burst.
  const chat = () =>
    page.evaluate(async (rawKey) => {
      const r = await fetch("/v1/chat/completions", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${rawKey}`,
        },
        body: JSON.stringify({
          model: "m-test",
          messages: [{ role: "user", content: "Say hi" }],
        }),
      });
      const body = r.headers.get("content-type")?.includes("json")
        ? await r.json()
        : await r.text();
      return { status: r.status, body };
    }, rawKey);

  let statuses: number[] = [];
  let first429Message = "";
  for (let attempt = 0; attempt < 2 && first429Message === ""; attempt++) {
    if (attempt === 1) {
      const now = new Date();
      const msIntoMinute = now.getSeconds() * 1000 + now.getMilliseconds();
      await page.waitForTimeout(62_000 - msIntoMinute);
    }
    statuses = [];
    for (let i = 0; i < 8; i++) {
      const r = await chat();
      statuses.push(r.status);
      if (r.status === 429 && !first429Message) {
        first429Message =
          ((r.body as { error?: { message?: string } }).error?.message ??
            "") as string;
      }
    }
  }
  console.log(`F1_BURST=${JSON.stringify(statuses)}`);
  console.log(`F1_429=${first429Message}`);
  expect(
    statuses.slice(0, 5),
    "the first 5 requests pass (impossible pre-fix)",
  ).toEqual([200, 200, 200, 200, 200]);
  expect(statuses.filter((s) => s === 429).length, "the cap bites").toBeGreaterThanOrEqual(1);
  expect(
    first429Message,
    "the denial names the REQUEST window",
  ).toMatch(/request/i);
  expect(first429Message).not.toMatch(/token/i);
});
