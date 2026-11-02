// Browser smoke for run-history rendering. API responses are fixtures; authorization is tested by the Rust integration suite.
import assert from "node:assert/strict";
import { chromium } from "playwright";
import { createServer } from "vite";

const ws = "00000000-0000-0000-0000-000000000001";
const sql = "SELECT * FROM sales.raw.orders";
const run = {
  id: "run-1", kind: "sql", state: "succeeded", title: sql, requested_by: "bob", on_behalf_of: "",
  target_id: "compute-1", created_ms: Date.now(), started_ms: Date.now(), finished_ms: Date.now(),
  error: "", exit_code: 0, cancelling: false, queued_ms: 30, run_ms: 40,
};
let restricted = false;
const unexpected = [];
const server = await createServer({ server: { host: "127.0.0.1", port: 0, strictPort: true } });
await server.listen();
const port = server.httpServer.address().port;
let browser;
try {
  browser = await chromium.launch({ headless: true, ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {}) });
  const page = await browser.newPage({ viewport: { width: 1360, height: 820 } });
  page.on("pageerror", (error) => unexpected.push(error.message));
  await page.addInitScript(() => sessionStorage.setItem("loamz-data-session", JSON.stringify({ access: "fixture-token", expires: Date.now() + 3600000 })));
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    let body;
    if (path === "/api/v1/me") {
      body = { subject: "bob", name: "Bob", email: "bob@example.test", kind: "person", groups: [], agent: null, orgs: [], workspaces: [{ id: ws, name: "Main", org_id: "acme", org_alias: "acme", admin: true }] };
    } else if (path === `/api/v1/workspaces/${ws}`) {
      body = { id: ws, name: "Main", org_id: "acme", admin: true, account_admin: true };
    } else if (path.endsWith("/securables")) {
      body = { items: [{ id: "cat-1", kind: "catalog", name: "Sales", owner: "user:bob", created_ms: Date.now() }] };
    } else if (path.endsWith("/queries")) {
      body = { items: [{ id: "query-1", name: "Sales query", owner: "user:bob", sql, target_id: "", created_ms: Date.now(), updated_ms: Date.now() }] };
    } else if (path.endsWith("/members")) {
      body = { items: [{ principal: "user:bob", role: "admin" }, { principal: "sp:agent", role: "user" }] };
    } else if (path.endsWith("/catalogs")) {
      body = { items: [] };
    } else if (path.endsWith("/entitlements")) {
      body = { items: [], available: [] };
    } else if (path.endsWith("/compute/targets")) {
      body = { items: [] };
    } else if (path.endsWith("/runs/run-1")) {
      body = restricted
        ? { ...run, result_access: "restricted", spec: { sql } }
        : { ...run, result_access: "available", spec: { sql }, stdout: "private output", stderr: "", result: { columns: [], rows: [], truncated: false } };
    } else if (path.endsWith("/runs")) {
      body = { items: [run] };
    } else {
      unexpected.push(`Unexpected API request: ${path}`);
      await route.fulfill({ status: 404, json: {} });
      return;
    }
    await route.fulfill({ status: 200, json: body });
  });
  const origin = `http://w-${ws}.localhost:${port}`;
  for (const [path, heading] of [["/", "Main"], ["/catalog", "Catalogs"], ["/queries", "Queries"], ["/jobs", "Jobs"], ["/compute", "Compute"], ["/settings", "Workspace settings"]]) {
    await page.goto(`${origin}${path}`);
    await page.getByRole("heading", { name: heading, exact: true }).waitFor();
  }
  await page.getByRole("combobox", { name: "Role of user:bob" }).waitFor();
  await page.goto(`${origin}/compute`);
  const sidebar = page.getByRole("navigation", { name: "Main" });
  const active = sidebar.getByRole("link", { name: "Compute" });
  const gaps = await active.evaluate((link) => {
    const nav = link.closest("nav").getBoundingClientRect();
    const item = link.getBoundingClientRect();
    return [item.left - nav.left, nav.right - item.right];
  });
  assert.ok(Math.abs(gaps[0] - gaps[1]) <= 2, `sidebar insets should match: ${gaps}`);
  await page.goto(`${origin}/settings`);
  await page.getByRole("combobox", { name: "Role of user:bob" }).waitFor();
  const member = await page.getByRole("combobox", { name: "Member", exact: true }).boundingBox();
  const entitlement = await page.getByRole("combobox", { name: "Entitlement", exact: true }).boundingBox();
  assert.ok(member && entitlement && Math.abs(member.y - entitlement.y) < 2, "desktop entitlement controls share one row");
  const section = page.getByRole("heading", { name: "Entitlements" }).locator("..").locator("..");
  assert.ok((await section.boundingBox()).height < 125, "entitlements section stays compact");
  if (process.env.LOAMZ_SCREENSHOTS) await page.screenshot({ path: `${process.env.LOAMZ_SCREENSHOTS}/loamz-settings-desktop.png` });
  await page.goto(`${origin}/queries`);
  await page.getByText("Sales query", { exact: true }).waitFor();
  await page.getByRole("textbox", { name: "Filter" }).fill("not present");
  await page.getByText("No matching queries.").waitFor();
  await page.goto(`${origin}/catalog`);
  await page.getByRole("textbox", { name: "Filter" }).fill("SALES");
  await page.getByRole("button", { name: "Sales" }).waitFor();
  await page.goto(`${origin}/sql`);
  await page.getByRole("button", { name: /run/i }).first().waitFor();
  await page.goto(`${origin}/history`);
  await page.getByText(sql, { exact: true }).first().click();
  await page.getByText("private output", { exact: true }).waitFor();
  if (process.env.LOAMZ_SCREENSHOTS) await page.screenshot({ path: `${process.env.LOAMZ_SCREENSHOTS}/loamz-history.png` });

  restricted = true;
  await page.goto(`${origin}/history`);
  await page.getByText(sql, { exact: true }).first().click();
  await page.getByText("Output is unavailable because access to the input data could not be verified.", { exact: true }).waitFor();
  assert.equal(await page.getByText(sql, { exact: true }).count(), 2, "history keeps the original statement");
  assert.equal(await page.getByText("private output", { exact: true }).count(), 0, "restricted output is not rendered");

  await page.goto(`${origin}/runs/run-1`);
  await page.getByText("Output is unavailable because access to the input data could not be verified.", { exact: true }).waitFor();
  assert.equal(await page.getByText("private output", { exact: true }).count(), 0);
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto(`${origin}/settings`);
  await page.getByRole("heading", { name: "Workspace settings" }).waitFor();
  if (process.env.LOAMZ_SCREENSHOTS) await page.screenshot({ path: `${process.env.LOAMZ_SCREENSHOTS}/loamz-settings-mobile.png` });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, "workspace fits a narrow viewport");
  assert.deepEqual(unexpected, []);
  console.log("Workspace screens, filtering and restricted output browser smoke passed (fixture API).");
} finally {
  await browser?.close();
  await server.close();
}
