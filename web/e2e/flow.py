"""Drives the web app as real people would, against a real empty server and the real local Keycloak (web/scripts/e2e.sh).
Everyone signs in on Keycloak's own page. People come from .dev/accounts.json (deploy/dev/seed.py)."""
import json, os, re, subprocess, sys, tempfile
from playwright.sync_api import sync_playwright, expect

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
acc = json.load(open(os.path.join(ROOT, ".dev", "accounts.json")))
SITE = "http://localhost:5180"
WS_HOST = re.compile(r"^http://w-[0-9a-f-]{36}\.localhost:5180/")
shots = os.environ.get("SHOTS")
problems, steps = [], 0
agent = None  # the compute agent this test starts, from the secret the console shows
CACHE = os.path.expanduser("~/.cache/loamz")


def step(name):
    global steps
    steps += 1
    print(f"  ok  {name}")


def sign_in(page, who):
    page.goto(SITE + "/")
    page.get_by_role("button", name="Sign in with Loamz").click()
    page.wait_for_selector("input[name=username]")
    page.fill("input[name=username]", who)
    if page.locator("input[name=password]").count() == 0:
        page.click("input[type=submit], button[type=submit]")  # identity-first: the password is asked on the next step
        page.wait_for_selector("input[name=password]")
    page.fill("input[name=password]", acc[who]["password"])
    page.click("input[type=submit], button[type=submit]")
    page.wait_for_url(SITE + "/**", timeout=20000)


def principal(page, kind, ident):
    page.get_by_label("Kind of principal").click()
    page.get_by_role("option", name=kind).click()
    page.get_by_label("Id", exact=True).fill(ident)


def shot(page, name):
    if shots:
        page.screenshot(path=f"{shots}/{name}.png")


with sync_playwright() as p:
    browser = p.chromium.launch()

    def person(who, **kw):
        ctx = browser.new_context(accept_downloads=True, **kw)
        page = ctx.new_page()
        page.on("console", lambda m: problems.append(m.text) if m.type == "error" else None)
        page.on("pageerror", lambda e: problems.append(f"PAGEERROR {e}"))
        sign_in(page, who)
        return page

    # -- ana: the account admin of acme
    ana = person("ana", viewport={"width": 1360, "height": 820})
    expect(ana.get_by_role("heading", name="Welcome, Ana")).to_be_visible(); step("signing in through Keycloak lands on Home")
    for thing in ("Catalog", "Workspace settings", "Workspace overview"):
        expect(ana.get_by_label("Main").get_by_label(thing)).to_have_count(0)
    step("outside a workspace the rail shows no workspace things")
    ana.reload(); expect(ana.get_by_role("heading", name="Welcome, Ana")).to_be_visible(); step("the session survives a reload")
    expect(ana.get_by_text("No Data account yet")).to_be_visible()
    ana.get_by_role("button", name="Create account").click()
    expect(ana.get_by_role("heading", name="Workspaces")).to_be_visible(); step("an account admin creates their organization's account")
    ana.get_by_placeholder("New workspace name").fill("Finance"); ana.get_by_role("button", name="Create", exact=True).click()
    expect(ana.get_by_text("Finance", exact=True)).to_be_visible(); step("and a workspace in it")
    shot(ana, "account-workspaces")

    ana.get_by_role("button", name="Open").click()
    ana.wait_for_url(WS_HOST, timeout=30000)
    expect(ana.get_by_role("heading", name="Finance")).to_be_visible(); step("opening a workspace goes to the workspace's own site, signed in without asking again")
    expect(ana.get_by_text("Jobs and runs")).to_be_visible(); expect(ana.get_by_text("SQL editor")).to_be_visible()
    step("its overview lists what can be done there")
    expect(ana.get_by_label("Main").get_by_label("Catalog")).to_be_visible(); step("and the rail has the workspace's own things")
    for out in ("Home", "All workspaces", "Account console"):
        expect(ana.get_by_label("Main").get_by_label(out)).to_have_count(0)
    ana.get_by_label("Account menu").click()
    expect(ana.get_by_role("menuitem", name="Sign out")).to_be_visible()
    expect(ana.get_by_role("menuitem")).to_have_count(1); step("inside a workspace the only way out is Sign out")
    ana.keyboard.press("Escape")
    assert ana.evaluate("[...document.querySelectorAll('a[href]')].every(a => new URL(a.href, location.href).origin === location.origin)"); step("and no link on the page leaves this site")
    ana.get_by_label("Main").get_by_label("Workspace settings").click()
    expect(ana.get_by_role("heading", name="Finance")).to_be_visible()
    principal(ana, "Group", "/finance/analysts"); ana.get_by_role("button", name="Assign").click()
    expect(ana.get_by_text("/finance/analysts").first).to_be_visible(); step("a Keycloak group is assigned to the workspace")
    shot(ana, "workspace-settings")

    # -- the catalog
    ana.get_by_label("Main").get_by_label("Catalog").click()
    ana.get_by_role("button", name="New", exact=True).click(); ana.get_by_placeholder("sales_2026").fill("Bad Name")
    expect(ana.get_by_role("button", name="Create")).to_be_disabled(); step("a name the server would refuse cannot be submitted")
    ana.get_by_placeholder("sales_2026").fill("sales"); ana.get_by_role("button", name="Create").click()
    expect(ana.get_by_role("heading", name="sales")).to_be_visible(); step("a catalog is created")
    ana.get_by_role("button", name="New", exact=True).click(); ana.get_by_placeholder("sales_2026").fill("raw"); ana.get_by_role("button", name="Create").click()
    expect(ana.get_by_role("heading", name="raw")).to_be_visible()
    ana.get_by_role("button", name="New", exact=True).click(); ana.get_by_label("Kind").click(); ana.get_by_role("option", name="Volume").click()
    ana.get_by_placeholder("sales_2026").fill("uploads"); ana.get_by_role("button", name="Create").click()
    expect(ana.get_by_role("heading", name="uploads")).to_be_visible(); step("a schema and a volume inside it")

    src = tempfile.NamedTemporaryFile(suffix=".txt", delete=False, mode="w"); src.write("hello from the browser"); src.close()
    name = os.path.basename(src.name)
    ana.locator("input[type=file]").set_input_files(src.name)
    expect(ana.get_by_text(name, exact=True)).to_be_visible(); step("an uploaded file is listed")
    with ana.expect_download() as dl:
        ana.get_by_label(f"Download {name}").click()
    assert open(dl.value.path()).read() == "hello from the browser"; step("the downloaded bytes are the uploaded bytes")
    shot(ana, "volume-files")

    ana.get_by_label("Main").get_by_label("Workspace overview").click()
    expect(ana.get_by_role("button", name="sales")).to_be_visible(); step("the overview lists the workspace's catalogs")
    ana.goto(SITE + "/")
    expect(ana.get_by_role("heading", name="Welcome, Ana")).to_be_visible(); step("the account site is its own address, and you are signed in there too")
    ana.get_by_role("button", name=re.compile("Finance")).first.click()
    ana.wait_for_url(WS_HOST, timeout=30000)
    ana.get_by_label("Main").get_by_label("Catalog").click()
    ana.get_by_role("button", name="sales", exact=True).click()

    # -- access: give the group read on the catalog
    ana.get_by_role("tab", name="Access").click()
    principal(ana, "Group", "/finance/analysts"); ana.get_by_role("button", name="Give access").click()
    expect(ana.get_by_text("SELECT").first).to_be_visible(); step("a group is given the Reader role on the catalog")
    shot(ana, "catalog-access")

    # -- account console: admins, activity, record check
    ana.goto(SITE + "/")
    ana.get_by_label("Account menu").click(); ana.get_by_role("menuitem", name="Account console").click()
    expect(ana.get_by_label("Main").get_by_label("Catalog")).to_have_count(0); step("the account console is on the account site, outside any workspace")
    ana.get_by_role("button", name="Account admins").click()
    expect(ana.get_by_text("Make admin")).to_be_visible(); step("the account admins are listed")
    ana.get_by_role("button", name="Activity").click()
    expect(ana.get_by_text("Created a catalog").first).to_be_visible(); step("activity shows what was done, in words")
    ana.get_by_role("button", name="Check the record").click()
    expect(ana.get_by_text(re.compile(r"Intact · \d+ entries"))).to_be_visible(); step("the organization's record is intact")
    shot(ana, "account-activity")

    # -- compute: attached in the account console, with a real agent started from the secret it shows
    ana.goto(SITE + "/")
    ana.get_by_label("Account menu").click(); ana.get_by_role("menuitem", name="Account console").click()
    ana.get_by_role("button", name="Compute", exact=True).click()
    expect(ana.get_by_text("No compute attached").first).to_be_visible(); step("an organization starts with no compute, and the console says so")
    ana.get_by_role("button", name="Attach compute").first.click()
    ana.get_by_placeholder("oracle").fill("this-machine"); ana.get_by_role("button", name="Attach", exact=True).click()
    expect(ana.get_by_text("Start the agent on this-machine")).to_be_visible()
    secret = re.search(r"agt_[0-9a-f]+", ana.locator("pre").first.inner_text()).group(0)
    secret_file = os.path.join(tempfile.mkdtemp(), "agent.secret"); open(secret_file, "w").write(secret); os.chmod(secret_file, 0o600)
    step("attaching compute shows its secret once, with the command to start the agent")
    env = {**os.environ, "LOAMZ_DATA_URL": "http://127.0.0.1:8020", "LOAMZ_AGENT_SECRET_FILE": secret_file, "LOAMZ_RUNSC": f"{CACHE}/runsc",
           "LOAMZ_ROOTFS": f"{CACHE}/rootfs", "LOAMZ_WORK": tempfile.mkdtemp(), "LOAMZ_PLATFORM": os.environ.get("LOAMZ_PLATFORM", "ptrace")}
    agent = subprocess.Popen([os.path.join(ROOT, "target/release/loamz-runner"), "agent"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ana.keyboard.press("Escape")
    expect(ana.get_by_text("online", exact=True).first).to_be_visible(timeout=30000); step("the machine shows as online once its agent connects out")
    ana.reload(); expect(ana.get_by_text("this-machine")).to_be_visible()
    expect(ana.get_by_text(secret)).to_have_count(0); step("and the secret is not shown again")

    # -- a table, with a few rows, and a query on it
    ana.goto(SITE + "/"); ana.get_by_role("button", name=re.compile("Finance")).first.click(); ana.wait_for_url(WS_HOST, timeout=30000)
    ana.get_by_label("Main").get_by_label("Catalog").click()
    ana.get_by_label("Expand sales").click(); ana.get_by_role("button", name="raw", exact=True).click()
    ana.get_by_role("button", name="New", exact=True).click()
    ana.get_by_label("Kind").click(); ana.get_by_role("option", name="Table").click()
    ana.get_by_placeholder("sales_2026").fill("orders"); ana.get_by_role("button", name="Create").click()
    expect(ana.get_by_role("heading", name="orders")).to_be_visible(); step("a table is created")
    rows = tempfile.NamedTemporaryFile(suffix=".csv", delete=False, mode="w"); rows.write("order,region,amount\n1,north,120\n2,south,80\n3,north,200\n4,east,50\n5,south,300\n"); rows.close()
    ana.locator("input[type=file]").set_input_files(rows.name)
    expect(ana.get_by_text(os.path.basename(rows.name), exact=True)).to_be_visible(); step("a CSV of five rows is uploaded to it")
    ana.get_by_role("button", name="Query").click()
    expect(ana.locator(".cm-content")).to_contain_text("SELECT * FROM sales.raw.orders LIMIT 100"); step("Query opens the SQL editor with a statement ready")
    ana.get_by_role("button", name="Run", exact=True).click()
    expect(ana.get_by_text("5 rows")).to_be_visible(timeout=60000); expect(ana.get_by_role("cell", name="north").first).to_be_visible(); step("running it shows the table's five rows")
    shot(ana, "sql-results")

    ana.locator(".cm-content").click(); ana.keyboard.press("Control+A")
    ana.keyboard.type("SELECT region, sum(amount) AS total FROM sales.raw.orders GROUP BY region ORDER BY total DESC")
    ana.keyboard.press("Control+Enter")
    expect(ana.get_by_text("3 rows")).to_be_visible(timeout=60000)
    assert ana.locator("tbody tr").first.inner_text().split() == ["1", "south", "380"]; step("Ctrl+Enter runs what was typed, and the sums are right")
    ana.locator(".cm-content").click(); ana.keyboard.press("Control+A"); ana.keyboard.type("SELECT * FROM read_csv('/etc/passwd')"); ana.keyboard.press("Control+Enter")
    expect(ana.get_by_text("table functions are not allowed")).to_be_visible(); step("a statement that reads a file is refused, in words")
    ana.locator(".cm-content").click(); ana.keyboard.press("Control+A"); ana.keyboard.type("SELECT nope FROM sales.raw.orders"); ana.keyboard.press("Control+Enter")
    expect(ana.get_by_text("The statement failed")).to_be_visible(timeout=60000); expect(ana.get_by_text("nope").first).to_be_visible(); step("a mistake in the SQL is shown as the database's own message")
    ana.get_by_role("button", name="History").click()
    expect(ana.get_by_text("SELECT region, sum(amount)").first).to_be_visible(); step("history lists what was run, newest first")
    ana.reload(); expect(ana.locator(".cm-content")).to_contain_text("SELECT nope"); step("the open tabs survive a reload")

    # -- a job: a script that reads the table and writes a file to a volume
    ana.get_by_label("Main").get_by_label("Compute").click()
    ana.get_by_role("button", name="New job").first.click()
    ana.get_by_label("Writes to").click() if False else None
    ana.locator("input[type=checkbox]").first.check()
    ana.get_by_role("combobox", name="Output volume").click(); ana.get_by_role("option").first.click()
    ana.locator(".cm-content").last.click(); ana.keyboard.press("Control+A")
    ana.keyboard.type("import csv, json\nrows = list(csv.DictReader(open('/inputs/orders/' + __import__('os').listdir('/inputs/orders')[0])))\nprint('total', sum(int(r['amount']) for r in rows))\nopen('/workspace/total.json', 'w').write(json.dumps({'rows': len(rows)}))")
    ana.get_by_role("button", name="Run job").click()
    expect(ana.get_by_text("succeeded").first).to_be_visible(timeout=90000)
    expect(ana.get_by_text("total 750")).to_be_visible(); step("a job reads the table in its own sandbox and its output is shown")
    expect(ana.get_by_text(re.compile(r"runs/[0-9a-f-]+/"))).to_be_visible(); step("and says where its files went")
    shot(ana, "job-detail")
    ana.keyboard.press("Escape")

    # -- cyd: in the group /finance/analysts, no organization role
    cyd = person("cyd", viewport={"width": 1360, "height": 820})
    expect(cyd.get_by_text("Finance", exact=True).first).to_be_visible(); step("a member of the assigned group sees the workspace on Home")
    expect(cyd.get_by_label("Main").get_by_label("Account console")).to_have_count(0); step("and no account console")
    cyd.get_by_text("Finance", exact=True).first.click()
    cyd.wait_for_url(WS_HOST, timeout=30000)
    expect(cyd.get_by_role("heading", name="Finance")).to_be_visible()
    expect(cyd.get_by_role("button", name="sales")).to_be_visible(); step("they open it and see the catalog the group was given on its overview")
    expect(cyd.get_by_text("Members, entitlements and catalogs")).to_have_count(0); step("without the admin's settings link")
    cyd.get_by_label("Main").get_by_label("Catalog").click()
    cyd.get_by_label("Expand sales").click(); cyd.get_by_label("Expand raw").click(); cyd.get_by_role("button", name="uploads").click()
    expect(cyd.get_by_text(name, exact=True)).to_be_visible(); step("and can read its files")
    cyd.locator("input[type=file]").set_input_files(src.name)
    expect(cyd.get_by_text("Could not upload")).to_be_visible(); step("but a reader's upload is refused with a clear message")
    cyd.get_by_role("tab", name="Access").click()
    expect(cyd.get_by_text("Only managers can see who has access")).to_be_visible(); step("and is told access is for managers")
    expect(cyd.get_by_label("Main").get_by_label("Workspace settings")).to_have_count(0); step("a non-admin has no workspace settings in the rail")
    cyd.goto(cyd.evaluate("location.origin") + "/settings")
    expect(cyd.get_by_text("Only admins can change this")).to_be_visible(); step("and typing the settings address gives them a plain refusal")
    shot(cyd, "reader-view")

    cyd.get_by_label("Main").get_by_label("SQL editor").click()
    cyd.locator(".cm-content").click(); cyd.keyboard.type("SELECT * FROM sales.raw.orders")
    cyd.keyboard.press("Control+Enter")
    expect(cyd.get_by_text("This could not be run")).to_be_visible(); expect(cyd.get_by_text("permission").first).to_be_visible(); step("someone without the SQL entitlement is refused, even though they can read the table")

    # -- the sign-in relay hands a code only to a workspace site of this account site
    evil = cyd.context.new_page()
    evil.goto(SITE + "/relay?code=x&state=n." + __import__("base64").urlsafe_b64encode(b"https://w-6f1c1a52-0000-4000-8000-000000000001.evil.example").decode().rstrip("="))
    evil.wait_for_url(SITE + "/", timeout=10000); step("the relay will not pass a sign-in code to a site that is not ours")
    evil.close()
    cyd.get_by_label("Account menu").click(); cyd.get_by_role("menuitem", name="Sign out").click()
    cyd.wait_for_url(re.compile(r"^http://localhost:5180/"), timeout=30000)
    expect(cyd.get_by_role("button", name="Sign in with Loamz").first).to_be_visible(); step("signing out of a workspace lands on the account site's front page")
    cyd.goto(cyd.url)  # the account site: still no session
    expect(cyd.get_by_role("button", name="Sign in with Loamz").first).to_be_visible()

    # -- bob: in the organization, not assigned anywhere
    bob = person("bob", viewport={"width": 1360, "height": 820})
    expect(bob.get_by_text("Finance")).to_have_count(0); step("someone not assigned does not see the workspace at all")

    # -- phone, dark theme, sign out
    m = person("ana", viewport={"width": 390, "height": 800}, has_touch=True)
    assert m.evaluate("document.documentElement.scrollWidth <= innerWidth"); step("nothing scrolls sideways on a phone")
    shot(m, "phone-home")
    ana.get_by_label("Switch to dark theme").click()
    assert ana.evaluate("document.documentElement.classList.contains('dark')"); ana.reload()
    assert ana.evaluate("document.documentElement.classList.contains('dark')"); step("the dark theme is remembered")
    shot(ana, "dark")
    ana.get_by_label("Account menu").click(); ana.get_by_role("menuitem", name="Sign out").click()
    ana.wait_for_url(re.compile(r".*(localhost:5180|127\.0\.0\.1:8082).*"))
    ana.goto(SITE + "/"); expect(ana.get_by_role("button", name="Sign in with Loamz")).to_be_visible(); step("signing out ends the session")
    browser.close()

if agent:
    agent.terminate()
real = [x for x in problems if "favicon" not in x and not re.search(r"\b(400|401|403|404|413)\b", x)]
print(f"\n{steps} steps passed; browser errors: {real or 'none'}")
sys.exit(1 if real else 0)
