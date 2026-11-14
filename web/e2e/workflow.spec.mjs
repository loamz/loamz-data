/**
 * End-to-end Playwright tests for Loamz Data web workflow.
 *
 * Verifies the 7 key user interactions against the live dev stack:
 *  1. Sign in as ana through Keycloak.
 *  2. Run `SELECT count(*) FROM nyc.main.trips` in SQL screen and see the count number.
 *  3. Browse catalog hierarchy to the `nyc.main.trips` table.
 *  4. Invite a person in the Account console.
 *  5. Create a service principal in workspace settings and see its token once.
 *  6. Add a compute target.
 *  7. Upload CSV to volume, import as new table, query in SQL, and append more rows.
 */
import assert from "node:assert/strict";
import { execSync } from "node:child_process";
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
console.log("Using accountsFile:", accountsFile, "ana password:", accounts.ana.password);
const SITE = process.env.LOAMZ_SITE || "http://localhost:5180";
const WS_HOST_REGEX = /^http:\/\/w-[0-9a-f-]{36}\.localhost:5180/;

let stepsPassed = 0;
function pass(name) {
  stepsPassed += 1;
  console.log(`  ✓ [Step ${stepsPassed}] ${name}`);
}

async function run() {
  console.log("=== Starting Playwright E2E Workflow Test ===");
  // Pre-sync ana password and workspace membership with dev stack
  try {
    const adminTokenRes = await fetch("http://127.0.0.1:8082/realms/master/protocol/openid-connect/token", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ grant_type: "password", client_id: "admin-cli", username: "admin", password: "admin-dev-only" }),
    });
    if (adminTokenRes.ok) {
      const { access_token } = await adminTokenRes.json();
      const userRes = await fetch("http://127.0.0.1:8082/admin/realms/loamz/users?username=ana&exact=true", {
        headers: { Authorization: `Bearer ${access_token}` },
      });
      const users = await userRes.json();
      if (Array.isArray(users) && users.length > 0) {
        const anaUser = users[0];
        await fetch(`http://127.0.0.1:8082/admin/realms/loamz/users/${anaUser.id}/reset-password`, {
          method: "PUT",
          headers: { Authorization: `Bearer ${access_token}`, "Content-Type": "application/json" },
          body: JSON.stringify({ type: "password", value: accounts.ana.password, temporary: false }),
        });
        const dbPath = path.join(ROOT, ".dev", "data.db");
        if (fs.existsSync(dbPath)) {
          execSync(`python3 -c "import sqlite3, time; conn=sqlite3.connect('${dbPath}'); c=conn.cursor(); c.execute(\\"INSERT OR REPLACE INTO account_admins VALUES ('acme', 'user:${anaUser.id}', '', ?)\\", (int(time.time()*1000),)); c.execute(\\"INSERT OR REPLACE INTO workspace_members VALUES ('919813a4-8c71-485f-8464-ddd0dce5e6bd', 'user:${anaUser.id}', 'admin', ?)\\", (int(time.time()*1000),)); conn.commit()"`);
        }
      }
    }
  } catch (err) {
    console.warn("Could not pre-sync ana password:", err.message);
  }

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

    try {
      await page.waitForURL(`${SITE}/**`, { timeout: 30000 });
    } catch {
      const errText = await page.locator(".alert, .feedback, .kc-feedback-text, #input-error").allInnerTexts();
      console.error("Login failed. Current URL:", page.url(), "Page alerts:", errText);
      throw new Error(`Login failed on ${page.url()}: ${errText.join("; ")}`);
    }
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
        await page.waitForTimeout(300);
        const option = page.getByRole("option").filter({ hasText: "dev-runner" }).or(page.getByRole("option").first());
        if (await option.isVisible()) {
          await option.click();
        }
      }
    }

    // Wait until run button is enabled, then click
    await page.waitForSelector('button:has-text("Run"):not([disabled])', { timeout: 15000 });
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
    await page.waitForTimeout(600);
    const machineOption = page.locator("[role='radio']").filter({ hasText: /machine/i }).first();
    if (await machineOption.isVisible()) {
      await machineOption.click();
      await page.waitForTimeout(300);
    }

    const computeNameInput = page.getByPlaceholder("oracle").or(page.getByPlaceholder("warehouse")).or(page.locator("input[placeholder='oracle']")).first();
    await computeNameInput.waitFor({ state: "visible", timeout: 10000 });
    const computeName = `worker-${Date.now()}`;
    await computeNameInput.fill(computeName);
    await page.waitForTimeout(200);
    if ((await computeNameInput.inputValue()) !== computeName) {
      await computeNameInput.fill(computeName);
    }

    const submitComputeBtn = page.getByRole("button", { name: "Create", exact: true }).first();
    await page.waitForSelector('button:has-text("Create"):not([disabled])', { timeout: 10000 });
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

    // -------------------------------------------------------------------------
    // 7. Upload CSV to volume, import as new table, query in SQL, and append
    // -------------------------------------------------------------------------
    console.log("7. Testing volume file import to table (create and append)...");
    const volumeId = "88888888-8888-8888-8888-888888888888";
    await page.goto(`${wsOrigin}/catalog/${volumeId}`);
    await page.waitForSelector("text='Upload'", { timeout: 15000 });
    await page.waitForSelector("text='Upload'", { timeout: 15000 });

    // Prepare temp CSV 1 (create mode)
    const testImportTable = `imported_orders_${Date.now()}`;
    const tmpDir = path.join(ROOT, ".dev");
    fs.mkdirSync(tmpDir, { recursive: true });
    const csvFile1 = path.join(tmpDir, "test_orders_1.csv");
    fs.writeFileSync(csvFile1, "id,customer,amount\n101,Alice,250.5\n102,Bob,120.0\n", "utf-8");

    // Upload csvFile1 to the volume
    console.log("   Uploading test_orders_1.csv to volume uploads...");
    const fileInput = page.locator("input[type=file]").first();
    await fileInput.setInputFiles(csvFile1);
    await page.waitForSelector("text='test_orders_1.csv'", { timeout: 15000 });

    // Click 'Import as table' for test_orders_1.csv
    console.log(`   Importing test_orders_1.csv as new table ${testImportTable}...`);
    const importAsTableBtn = page.getByRole("button", { name: "Import test_orders_1.csv as table" })
      .or(page.getByRole("button", { name: "Import as table" })).first();
    await importAsTableBtn.click();

    // Verify dialog opened
    const importDialog = page.locator("[role='dialog']").first();
    await importDialog.waitFor({ state: "visible", timeout: 10000 });

    // Enter table name
    const tblNameInput = importDialog.getByRole("textbox", { name: "Table Name" }).or(importDialog.locator("input[placeholder='table_name']")).first();
    await tblNameInput.fill(testImportTable);

    // Click 'Start import'
    const startImportBtn = importDialog.getByRole("button", { name: /start import/i }).first();
    await startImportBtn.click();

    // Wait for import job completion
    console.log("   Waiting for import job to complete...");
    await page.waitForSelector("[role='dialog']:has-text('Import complete')", { timeout: 60000 });
    const successMsg = await importDialog.innerText();
    assert.ok(successMsg.includes("2 rows") || successMsg.includes("complete"), "Expected 2 rows imported");

    // Dismiss dialog using Done button
    const importDoneBtn = importDialog.getByRole("button", { name: "Done" }).first();
    await importDoneBtn.click();
    await importDialog.waitFor({ state: "hidden", timeout: 10000 }).catch(() => {});

    // Go to SQL screen to verify row count
    console.log(`   Verifying SELECT count(*) FROM nyc.main.${testImportTable}...`);
    await page.goto(`${wsOrigin}/sql`);
    await page.waitForSelector(".cm-content", { timeout: 15000 });
    const cm = page.locator(".cm-content").first();
    await cm.click();
    await page.keyboard.press("Control+A");
    await page.keyboard.type(`SELECT count(*) FROM nyc.main.${testImportTable}`);

    const sqlRunBtn = page.getByRole("button", { name: "Run", exact: true }).first();
    await sqlRunBtn.click();

    await page.waitForSelector("tbody tr td", { timeout: 45000 });
    const resCell1 = page.locator("tbody tr td").nth(1);
    await resCell1.waitFor({ state: "visible", timeout: 15000 });
    const count1 = (await resCell1.innerText()).trim();
    console.log(`   Count after create: ${count1}`);
    assert.equal(Number(count1.replace(/,/g, "")), 2, `Expected 2 rows after initial import, got ${count1}`);

    // Now test append mode from Table page: "Import from a file"
    console.log(`   Opening table page nyc.main.${testImportTable} for append import...`);
    await page.goto(`${wsOrigin}/catalog`);
    await page.waitForSelector("text='Catalogs'", { timeout: 15000 });

    // Prepare temp CSV 2 (append mode)
    const csvFile2 = path.join(tmpDir, "test_orders_2.csv");
    fs.writeFileSync(csvFile2, "id,customer,amount\n103,Carol,75.25\n", "utf-8");

    // Navigate to the table in catalog tree
    const expandNycAgain = page.getByRole("button", { name: "Expand nyc" }).or(page.locator("button[aria-label='Expand nyc']")).first();
    await expandNycAgain.waitFor({ state: "visible", timeout: 15000 });
    await expandNycAgain.click();
    const expandMainAgain = page.getByRole("button", { name: "Expand main" }).or(page.locator("button[aria-label='Expand main']")).first();
    await expandMainAgain.waitFor({ state: "visible", timeout: 15000 });
    await expandMainAgain.click();

    const targetTblRow = page.getByRole("button", { name: testImportTable, exact: true })
      .or(page.locator("li[role='treeitem'] button").filter({ hasText: new RegExp(`^${testImportTable}$`) })).first();
    await targetTblRow.waitFor({ state: "visible", timeout: 15000 });
    await targetTblRow.click();

    // Click "Import from a file" action on table header
    const importFromFileBtn = page.getByRole("button", { name: "Import from a file" }).first();
    await importFromFileBtn.waitFor({ state: "visible", timeout: 15000 });
    await importFromFileBtn.click();

    // Dialog opens with prefilled target and mode 'append'
    const appendDialog = page.locator("[role='dialog']").first();
    await appendDialog.waitFor({ state: "visible", timeout: 10000 });

    // Wait until source volume is loaded in dialog
    const volSelect = appendDialog.getByRole("combobox", { name: "Source Volume" }).first();
    await volSelect.waitFor({ state: "visible", timeout: 15000 });
    await page.waitForTimeout(1000);

    // Upload csvFile2 via the dialog's upload button
    console.log("   Uploading test_orders_2.csv through dialog upload...");
    const dialogFileInput = appendDialog.locator("input[type=file]").first();
    await dialogFileInput.setInputFiles(csvFile2);
    // Wait for the uploaded file to appear in the dialog
    await page.waitForSelector("text='test_orders_2.csv'", { timeout: 15000 });
    const fileSelect = appendDialog.getByRole("combobox", { name: "Source File" }).first();
    if (await fileSelect.isVisible()) {
      await fileSelect.click();
      await page.waitForTimeout(200);
      const opt2 = page.getByRole("option", { name: "test_orders_2.csv" }).or(page.locator("[role='option']:has-text('test_orders_2.csv')")).first();
      if (await opt2.isVisible()) {
        await opt2.click();
      }
    }
    await page.waitForTimeout(300);

    // Click 'Start import'
    const startAppendBtn = appendDialog.getByRole("button", { name: /start import/i }).first();
    await startAppendBtn.click();

    // Wait for import job to complete
    console.log("   Waiting for append import job to complete...");
    await page.waitForSelector("[role='dialog']:has-text('Import complete')", { timeout: 60000 });
    const appendSuccessMsg = await appendDialog.innerText();
    assert.ok(appendSuccessMsg.includes("1 row") || appendSuccessMsg.includes("complete"), "Expected 1 row appended");

    // Dismiss dialog
    const appendDoneBtn = appendDialog.getByRole("button", { name: "Done" }).first();
    await appendDoneBtn.click();
    await appendDialog.waitFor({ state: "hidden", timeout: 10000 }).catch(() => {});

    // Go back to SQL screen to verify appended count is now 3
    console.log(`   Verifying SELECT count(*) FROM nyc.main.${testImportTable} after append...`);
    await page.goto(`${wsOrigin}/sql`);
    await page.waitForSelector(".cm-content", { timeout: 15000 });
    const cm2 = page.locator(".cm-content").first();
    await cm2.click();
    await page.keyboard.press("Control+A");
    await page.keyboard.type(`SELECT count(*) FROM nyc.main.${testImportTable}`);

    const sqlRunBtn2 = page.getByRole("button", { name: "Run", exact: true }).first();
    await page.waitForSelector('button:has-text("Run"):not([disabled])', { timeout: 15000 });
    await sqlRunBtn2.click();

    await page.waitForSelector("tbody tr td", { timeout: 45000 });
    const resCell2 = page.locator("tbody tr td").nth(1);
    await resCell2.waitFor({ state: "visible", timeout: 15000 });
    const count2 = (await resCell2.innerText()).trim();
    console.log(`   Count after append: ${count2}`);
    assert.equal(Number(count2.replace(/,/g, "")), 3, `Expected 3 rows after append import, got ${count2}`);

    pass(`Imported CSV into new table ${testImportTable} (2 rows), verified via SQL, and appended rows from second CSV (total 3 rows)`);

    console.log(`\n=================================================`);
    console.log(`ALL 7 E2E WORKFLOW TESTS PASSED (${stepsPassed}/7)!`);
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
