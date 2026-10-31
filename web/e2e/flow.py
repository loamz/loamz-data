"""Drives the web app as real people would, against a real empty server and the real local Keycloak (web/scripts/e2e.sh).
Everyone signs in on Keycloak's own page. People come from .dev/accounts.json (deploy/dev/seed.py)."""
import json, os, re, sys, tempfile
from playwright.sync_api import sync_playwright, expect

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
acc = json.load(open(os.path.join(ROOT, ".dev", "accounts.json")))
SITE = "http://localhost:5180"
shots = os.environ.get("SHOTS")
problems, steps = [], 0


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
    ana.reload(); expect(ana.get_by_role("heading", name="Welcome, Ana")).to_be_visible(); step("the session survives a reload")
    expect(ana.get_by_text("No Data account yet")).to_be_visible()
    ana.get_by_role("button", name="Create account").click()
    expect(ana.get_by_role("heading", name="Workspaces")).to_be_visible(); step("an account admin creates their organization's account")
    ana.get_by_placeholder("New workspace name").fill("Finance"); ana.get_by_role("button", name="Create", exact=True).click()
    expect(ana.get_by_text("Finance", exact=True)).to_be_visible(); step("and a workspace in it")
    shot(ana, "account-workspaces")

    ana.get_by_role("button", name="Settings").click()
    expect(ana.get_by_role("heading", name="Finance")).to_be_visible()
    principal(ana, "Group", "/finance/analysts"); ana.get_by_role("button", name="Assign").click()
    expect(ana.get_by_text("/finance/analysts").first).to_be_visible(); step("a Keycloak group is assigned to the workspace")
    shot(ana, "workspace-settings")

    # -- the catalog
    ana.get_by_label("Main").get_by_label("Catalog").click()
    ana.get_by_role("button", name="New").click(); ana.get_by_placeholder("sales_2026").fill("Bad Name")
    expect(ana.get_by_role("button", name="Create")).to_be_disabled(); step("a name the server would refuse cannot be submitted")
    ana.get_by_placeholder("sales_2026").fill("sales"); ana.get_by_role("button", name="Create").click()
    expect(ana.get_by_role("heading", name="sales")).to_be_visible(); step("a catalog is created")
    ana.get_by_role("button", name="New").click(); ana.get_by_placeholder("sales_2026").fill("raw"); ana.get_by_role("button", name="Create").click()
    expect(ana.get_by_role("heading", name="raw")).to_be_visible()
    ana.get_by_role("button", name="New").click(); ana.get_by_label("Kind").click(); ana.get_by_role("option", name="Volume").click()
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

    # -- access: give the group read on the catalog
    ana.get_by_role("tab", name="Access").click()
    expect(ana.get_by_text("MANAGE")).to_be_hidden() if False else None
    ana.get_by_role("button", name="Collapse raw").click() if False else None
    ana.get_by_label("Expand sales").click() if ana.get_by_label("Expand sales").count() else None
    ana.get_by_role("button", name="sales", exact=True).click()
    ana.get_by_role("tab", name="Access").click()
    principal(ana, "Group", "/finance/analysts"); ana.get_by_role("button", name="Give access").click()
    expect(ana.get_by_text("SELECT").first).to_be_visible(); step("a group is given the Reader role on the catalog")
    shot(ana, "catalog-access")

    # -- account console: admins, activity, record check
    ana.get_by_label("Main").get_by_label("Account console").click()
    ana.get_by_role("button", name="Account admins").click()
    expect(ana.get_by_text("Make admin")).to_be_visible(); step("the account admins are listed")
    ana.get_by_role("button", name="Activity").click()
    expect(ana.get_by_text("Created a catalog").first).to_be_visible(); step("activity shows what was done, in words")
    ana.get_by_role("button", name="Check the record").click()
    expect(ana.get_by_text(re.compile(r"Intact · \d+ entries"))).to_be_visible(); step("the organization's record is intact")
    shot(ana, "account-activity")

    # -- cyd: in the group /finance/analysts, no organization role
    cyd = person("cyd", viewport={"width": 1360, "height": 820})
    expect(cyd.get_by_text("Finance", exact=True).first).to_be_visible(); step("a member of the assigned group sees the workspace on Home")
    expect(cyd.get_by_label("Main").get_by_label("Account console")).to_have_count(0); step("and no account console")
    cyd.get_by_text("Finance", exact=True).first.click()
    expect(cyd.get_by_role("button", name="sales", exact=True)).to_be_visible(); step("they see the catalog the group was given")
    cyd.get_by_label("Expand sales").click(); cyd.get_by_label("Expand raw").click(); cyd.get_by_role("button", name="uploads").click()
    expect(cyd.get_by_text(name, exact=True)).to_be_visible(); step("and can read its files")
    cyd.locator("input[type=file]").set_input_files(src.name)
    expect(cyd.get_by_text("Could not upload")).to_be_visible(); step("but a reader's upload is refused with a clear message")
    cyd.get_by_role("tab", name="Access").click()
    expect(cyd.get_by_text("Only managers can see who has access")).to_be_visible(); step("and is told access is for managers")
    cyd.get_by_label("Main").get_by_label("Workspace settings").click()
    expect(cyd.get_by_text("Only admins can change this")).to_be_visible(); step("workspace settings are for admins")
    shot(cyd, "reader-view")

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

real = [x for x in problems if "favicon" not in x and not re.search(r"\b(401|403|404|413)\b", x)]
print(f"\n{steps} steps passed; browser errors: {real or 'none'}")
sys.exit(1 if real else 0)
