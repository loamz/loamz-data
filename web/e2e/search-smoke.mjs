import assert from "node:assert/strict";
import { chromium } from "playwright";
import { createServer } from "vite";

const ws = "00000000-0000-0000-0000-000000000001";
const unexpected = [];
const queries = [];
let markOldStarted;
let finishOldResponse;
const oldStarted = new Promise((resolve) => { markOldStarted = resolve; });
const oldResponse = new Promise((resolve) => { finishOldResponse = resolve; });
const server = await createServer({ server: { host: "127.0.0.1", port: 0, strictPort: true } });
await server.listen();
const port = server.httpServer.address().port;
let browser;
try {
  browser = await chromium.launch({ headless: true, ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {}) });
  const page = await browser.newPage();
  page.on("pageerror", (error) => unexpected.push(error.message));
  await page.addInitScript(() => sessionStorage.setItem("loamz-data-session", JSON.stringify({ access: "fixture-token", expires: Date.now() + 3600000 })));
  await page.route("**/api/v1/**", async (route) => {
    const url = new URL(route.request().url());
    let body;
    if (url.pathname === "/api/v1/me") {
      body = { subject: "bob", name: "Bob", email: "bob@example.test", kind: "person", groups: [], agent: null, orgs: [], workspaces: [{ id: ws, name: "Main", org_id: "acme", org_alias: "acme", admin: true }] };
    } else if (url.pathname === `/api/v1/workspaces/${ws}`) {
      body = { id: ws, name: "Main", org_id: "acme", admin: true, account_admin: true };
    } else if (url.pathname.endsWith("/search")) {
      const query = url.searchParams.get("q");
      queries.push(query);
      if (query === "OLD") {
        markOldStarted();
        await oldResponse;
      }
      const name = query === "OLD" ? "old" : query === "NEW" ? "new" : "orders";
      body = { items: [{ id: `table-${name}`, kind: "table", parent_id: "schema-1", name, description: null, tags: [], owner: "user:bob", classification: "normal", placement: "local", consistency: "transaction", created_ms: Date.now(), path: `Sales.raw.${name}` }] };
    } else if (url.pathname.endsWith("/securables")) {
      body = { items: [{ id: "catalog-1", kind: "catalog", parent_id: "", name: "sales", description: null, tags: [], owner: "user:bob", classification: "normal", placement: "local", consistency: "transaction", created_ms: Date.now() }] };
    } else {
      unexpected.push(`Unexpected API request: ${url.pathname}`);
      await route.fulfill({ status: 404, json: {} });
      return;
    }
    await route.fulfill({ status: 200, json: body });
  });
  await page.goto(`http://w-${ws}.localhost:${port}/catalog`);
  await page.getByRole("heading", { name: "Catalogs", exact: true }).waitFor();
  await page.getByRole("textbox", { name: "Filter" }).fill("ORDERS");
  await page.getByRole("button", { name: "Sales.raw.orders" }).waitFor();
  assert.ok(queries.includes("ORDERS"), "the filter calls workspace search");
  const filter = page.getByRole("textbox", { name: "Filter" });
  await filter.fill("OLD");
  await oldStarted;
  await filter.fill("NEW");
  await page.getByRole("button", { name: "Sales.raw.new" }).waitFor();
  finishOldResponse();
  await page.waitForTimeout(50);
  assert.equal(await page.getByRole("button", { name: "Sales.raw.old" }).count(), 0, "a stale response cannot replace newer results");
  assert.deepEqual(unexpected, []);
  console.log("Catalog search browser smoke passed.");
} finally {
  await browser?.close();
  await server.close();
}
