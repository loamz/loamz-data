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
      body = { subject: "bob", name: "Bob", email: "bob@example.test", kind: "person", groups: [], agent: null, orgs: [], workspaces: [{ id: ws, name: "Main", org_id: "acme", org_alias: "acme", admin: false }] };
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
  await page.goto(`${origin}/history`);
  await page.getByText(sql, { exact: true }).first().click();
  await page.getByText("private output", { exact: true }).waitFor();

  restricted = true;
  await page.goto(`${origin}/history`);
  await page.getByText(sql, { exact: true }).first().click();
  await page.getByText("Output is unavailable because access to the input data could not be verified.", { exact: true }).waitFor();
  assert.equal(await page.getByText(sql, { exact: true }).count(), 2, "history keeps the original statement");
  assert.equal(await page.getByText("private output", { exact: true }).count(), 0, "restricted output is not rendered");

  await page.goto(`${origin}/runs/run-1`);
  await page.getByText("Output is unavailable because access to the input data could not be verified.", { exact: true }).waitFor();
  assert.equal(await page.getByText("private output", { exact: true }).count(), 0);
  assert.deepEqual(unexpected, []);
  console.log("Run history and restricted output browser smoke passed (fixture API).");
} finally {
  await browser?.close();
  await server.close();
}
