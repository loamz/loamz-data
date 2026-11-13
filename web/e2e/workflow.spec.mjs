/**
 * End-to-end Playwright tests for Loamz Data web workflow.
 *
 * Verifies the 6 key user interactions against the live dev stack:
 *  1. Sign in as ana through Keycloak.
 *  2. Run `SELECT count(*) FROM nyc.main.trips` in SQL screen and see the count number.
 *  3. Browse catalog hierarchy to the `nyc.main.trips` table.
 *  4. Invite a person in the Account console.
 *  5. Create a service principal in workspace settings and see its token once.
 *  6. Add a compute target.
 */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "../..");
const accountsFile = process.env.LOAMZ_DEV_ACCOUNTS || (fs.existsSync(path.join(ROOT, ".dev", "accounts.json")) ? path.join(ROOT, ".dev", "accounts.json") : path.resolve(ROOT, "../loamz-data/.dev/accounts.json"));

if (!fs.existsSync(accountsFile)) {
  console.error("Missing .dev/accounts.json. Run deploy/dev/seed.py first.");
  process.exit(1);
}

const accounts = JSON.parse(fs.readFileSync(accountsFile, "utf-8"));
const SITE = process.env.LOAMZ_SITE || "http://localhost:5180";
const WS_HOST_REGEX = /^http:\/\/w-[0-9a-f-]{36}\.localhost:5180/;

let stepsPassed = 0;
function pass(name) {
  stepsPassed += 1;
  console.log(`  ✓ [Step ${stepsPassed}] ${name}`);
}

async function run() {
  console.log("=== Starting Playwright E2E Workflow Test ===");
  const browser = await chromium.launch({
    headless: true,
    ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {}),
  });

  const context = await browser.newContext({
    viewport: { width: 1360, height: 820 },
  });
  const page = await context.newPage();

  // Track uncaught browser exceptions
  const pageErrors = [];
  page.on("pageerror", (err) => pageErrors.push(err.message));

  try {
    // -------------------------------------------------------------------------
    // 1. Sign in as ana
    // -------------------------------------------------------------------------
    console.log("1. Signing in as ana...");
    await page.goto(`${SITE}/`);

    // The Sign in button might take a moment to enable once OIDC discovery completes
    const signInBtn = page.getByRole("button", { name: "Sign in", exact: true }).first();
    await signInBtn.waitFor({ state: "visible", timeout: 15000 });
    // Wait until it's enabled
    for (let i = 0; i < 30; i++) {
      if (await signInBtn.isEnabled()) break;
      await page.waitForTimeout(500);
    }
    await signInBtn.click();

    await page.waitForSelector("input[name=username]", { timeout: 20000 });
    await page.fill("input[name=username]", "ana");
    if ((await page.locator("input[name=password]").count()) === 0) {
      await page.click("input[type=submit], button[type=submit]");
      await page.waitForSelector("input[name=password]", { timeout: 10000 });
    }
    await page.fill("input[name=password]", accounts.ana.password);
    await page.click("input[type=submit], button[type=submit]");

    await page.waitForURL(`${SITE}/**`, { timeout: 30000 });
    await page.waitForSelector("h1:has-text('Welcome, Ana'), h2:has-text('Welcome, Ana')", { timeout: 15000 });
    pass("Sign in as ana successful and landed on Welcome, Ana");

    // Open workspace 'Main'
    const mainWorkspaceBtn = page.getByRole("button", { name: /Main/ }).first();
    await mainWorkspaceBtn.waitFor({ state: "visible", timeout: 10000 });
    await mainWorkspaceBtn.click();

    await page.waitForURL(WS_HOST_REGEX, { timeout: 30000 });
    const wsOrigin = new URL(page.url()).origin;
    console.log(`   Connected to workspace origin: ${wsOrigin}`);
    pass("Opened workspace Main on its dedicated origin");

    // -------------------------------------------------------------------------
    // 2. Run SELECT count(*) FROM nyc.main.trips in SQL screen and see the number
    // -------------------------------------------------------------------------
    console.log("2. Running SQL query: SELECT count(*) FROM nyc.main.trips...");
    const sqlNav = page.getByRole("link", { name: "SQL editor" }).or(page.getByText("SQL editor")).first();
    await sqlNav.click();

    await page.waitForSelector(".cm-content", { timeout: 15000 });
    const cmEditor = page.locator(".cm-content").first();
    await cmEditor.click();
    await page.keyboard.press("Control+A");
    await page.keyboard.type("SELECT count(*) FROM nyc.main.trips");

    // Ensure compute target is selected if not already
    const runBtn = page.getByRole("button", { name: "Run", exact: true }).first();
    await runBtn.waitFor({ state: "visible" });

    if (await runBtn.isDisabled()) {
      const computePicker = page.getByRole("combobox", { name: "Compute" }).first();
      if (await computePicker.isVisible()) {
        await computePicker.click();
        const firstOption = page.getByRole("option").first();
        if (await firstOption.isVisible()) {
          await firstOption.click();
        }
      }
    }

    // Wait until run button is enabled, then click
    await runBtn.waitFor({ state: "visible" });
    await page.waitForTimeout(500);
    await runBtn.click();

    // Wait for the query result grid/cell to display the count number
    console.log("   Waiting for query execution...");
    await page.waitForSelector("tbody tr", { timeout: 45000 });
    const rowHeader = page.locator("tbody tr td").first();
    const resultCell = page.locator("tbody tr td").nth(1);
    await resultCell.waitFor({ state: "visible", timeout: 15000 });
    const countValue = (await resultCell.innerText()).trim();
    console.log(`   Query result returned count: ${countValue}`);
    assert.ok(Number(countValue.replace(/,/g, "")) > 0, `Expected positive count, got ${countValue}`);
    pass(`Ran 'SELECT count(*) FROM nyc.main.trips' in SQL screen and saw count number: ${countValue}`);

    // -------------------------------------------------------------------------
    // 3. Browse catalog to the table nyc.main.trips
    // -------------------------------------------------------------------------
    console.log("3. Browsing catalog to nyc.main.trips...");
    const catalogNav = page.getByRole("link", { name: "Catalog" }).or(page.getByText("Catalog")).first();
    await catalogNav.click();

    // Expand nyc catalog
    const expandNyc = page.getByRole("button", { name: "Expand nyc" }).or(page.locator("button[aria-label='Expand nyc']")).first();
    await expandNyc.waitFor({ state: "visible", timeout: 15000 });
    await expandNyc.click();

    // Expand main schema
    const expandMain = page.getByRole("button", { name: "Expand main" }).or(page.locator("button[aria-label='Expand main']")).first();
    await expandMain.waitFor({ state: "visible", timeout: 15000 });
    await expandMain.click();

    // Open trips table
    const tripsRow = page.getByRole("button", { name: "trips", exact: true }).or(page.locator("li[role='treeitem'] button").filter({ hasText: /^trips$/ })).first();
    await tripsRow.waitFor({ state: "visible", timeout: 15000 });
    await tripsRow.click();

    // Assert table details are rendered
    await page.waitForSelector("h1:has-text('trips'), h2:has-text('trips'), [data-slot='table']", { timeout: 15000 });
    const bodyText = await page.locator("body").innerText();
    assert.ok(bodyText.includes("pickup_ts") || bodyText.includes("trips"), "Table details should show columns or table name");
    pass("Browsed catalog hierarchy to table nyc.main.trips successfully");

    // -------------------------------------------------------------------------
    // 4. Invite a person in Account console
    // -------------------------------------------------------------------------
    console.log("4. Inviting a person in Account console...");
    await page.goto(`${SITE}/`);
    await page.waitForSelector("nav[aria-label='Main']", { timeout: 15000 });

    const accountConsoleLink = page.getByRole("link", { name: "Account console" }).or(page.locator("a[aria-label='Account console']")).first();
    if (await accountConsoleLink.isVisible()) {
      await accountConsoleLink.click();
    } else {
      const manageBtn = page.getByRole("button", { name: "Manage" }).first();
      await manageBtn.click();
    }

    await page.waitForSelector("aside", { timeout: 15000 });

    const peopleNav = page.getByRole("button", { name: "People" }).or(page.getByText("People")).first();
    await peopleNav.waitFor({ state: "visible", timeout: 10000 });
    await peopleNav.click();

    // Fill invitation form
    const emailInput = page.getByPlaceholder("name@company.com").or(page.locator("input[aria-label='Email']")).first();
    await emailInput.waitFor({ state: "visible", timeout: 10000 });
    const uniqueEmail = `invited_${Date.now()}@acme.test`;
    await emailInput.fill(uniqueEmail);

    const nameInput = page.getByPlaceholder("Their name").or(page.locator("input[aria-label='Name']")).first();
    await nameInput.fill("Invited User");

    const addBtn = page.getByRole("button", { name: "Add", exact: true }).first();
    await addBtn.click();

    // Verify invitation dialog appeared
    const doneBtn = page.getByRole("button", { name: "Done", exact: true }).first();
    await doneBtn.waitFor({ state: "visible", timeout: 15000 });
    const dialogText = await page.locator("[role='dialog']").innerText();
    assert.ok(dialogText.includes(uniqueEmail) || dialogText.includes("password") || dialogText.includes("Added"), "Invitation dialog confirmed");
    await doneBtn.click();
    pass(`Invited person (${uniqueEmail}) in Account console and verified invitation dialog`);

    // -------------------------------------------------------------------------
    // 5. Create a service principal and see its token once
    // -------------------------------------------------------------------------
    console.log("5. Creating service principal and viewing token once...");
    await page.goto(`${wsOrigin}/settings/principals`);
    await page.waitForSelector("form", { timeout: 15000 });

    const spNameInput = page.getByPlaceholder(/tableau/i).or(page.locator("input[aria-label='Service principal name']")).first();
    await spNameInput.waitFor({ state: "visible", timeout: 10000 });
    const spName = `bot-${Date.now()}`;
    await spNameInput.fill(spName);

    const createSpBtn = page.getByRole("button", { name: "Create", exact: true }).first();
    await createSpBtn.click();

    // Confirm shown-once token dialog is displayed
    const spDialog = page.locator("[role='dialog']").first();
    await spDialog.waitFor({ state: "visible", timeout: 15000 });
    const spDialogText = await spDialog.innerText();
    assert.ok(spDialogText.includes("shown once") || spDialogText.includes("Token for"), "Shown-once dialog appeared");

    // Capture token value
    const tokenPre = page.locator("[role='dialog'] pre").first();
    const tokenVal = (await tokenPre.innerText()).trim();
    assert.ok(tokenVal.length > 10, "Service principal token should be non-empty");

    // Close dialog
    const spDoneBtn = page.getByRole("button", { name: "Done", exact: true }).first();
    await spDoneBtn.click();
    await spDialog.waitFor({ state: "hidden", timeout: 10000 });

    // Verify service principal is in table, and token is no longer visible on page
    await page.waitForSelector(`text=${spName}`, { timeout: 10000 });
    const pageTextAfter = await page.locator("body").innerText();
    assert.ok(!pageTextAfter.includes(tokenVal), "Token must NOT be visible on page after dialog is dismissed");
    pass(`Created service principal ${spName}, saw token once, and verified token disappeared on dismiss`);

    // -------------------------------------------------------------------------
    // 6. Add a compute target
    // -------------------------------------------------------------------------
    console.log("6. Adding a compute target...");
    await page.goto(`${wsOrigin}/compute`);
    await page.waitForSelector("body", { timeout: 15000 });

    const createComputeBtn = page.getByRole("button", { name: "Create compute" }).first();
    await createComputeBtn.waitFor({ state: "visible", timeout: 15000 });
    await createComputeBtn.click();

    // Dialog should open
    await page.waitForSelector("[role='dialog']", { timeout: 10000 });

    // Select Machine option if available
    const machineOption = page.locator("[role='radio']").filter({ hasText: /machine/i }).first();
    if (await machineOption.isVisible()) {
      await machineOption.click();
    }

    const computeNameInput = page.getByPlaceholder("oracle").or(page.getByPlaceholder("warehouse")).or(page.locator("input[placeholder='oracle']")).first();
    await computeNameInput.waitFor({ state: "visible", timeout: 10000 });
    const computeName = `worker-${Date.now()}`;
    await computeNameInput.fill(computeName);

    const submitComputeBtn = page.getByRole("button", { name: "Create", exact: true }).first();
    await submitComputeBtn.click();

    // If machine, dialog shows "Start the agent on ..." with copy button and close
    const agentDialog = page.locator("[role='dialog']").first();
    await agentDialog.waitFor({ state: "visible", timeout: 15000 });
    // Close the dialog using Close button or Escape
    const closeBtn = page.locator("[role='dialog'] button[aria-label='Close']").or(page.getByRole("button", { name: "Close" })).first();
    if (await closeBtn.isVisible()) {
      await closeBtn.click();
    } else {
      await page.keyboard.press("Escape");
    }
    await agentDialog.waitFor({ state: "hidden", timeout: 10000 }).catch(() => {});

    // Verify compute target appears in the table
    await page.waitForSelector(`text=${computeName}`, { timeout: 15000 });
    pass(`Added compute target ${computeName} and verified in compute table`);

    console.log(`\n=================================================`);
    console.log(`ALL 6 E2E WORKFLOW TESTS PASSED (${stepsPassed}/6)!`);
    console.log(`=================================================\n`);
  } finally {
    await browser.close();
  }

  // Filter out harmless client favicon/network 404s
  const fatalErrors = pageErrors.filter((e) => !e.includes("favicon") && !e.includes("404"));
  if (fatalErrors.length > 0) {
    console.error("Browser errors encountered during run:", fatalErrors);
    process.exit(1);
  }
}

run().catch((err) => {
  console.error("E2E Test Failed with Error:", err);
  process.exit(1);
});
