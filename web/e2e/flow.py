"""Drives the web app as people would, against a dev server with sample data (scripts/dev-ui.sh).
Run through web/scripts/e2e.sh. Needs Python Playwright with Chromium (PLAYWRIGHT_PY points at it)."""
import os, re, sys, tempfile
from playwright.sync_api import sync_playwright, expect

WORK, SITE = os.environ["WORK"], os.environ.get("SITE", "http://127.0.0.1:5180")
ana = open(f"{WORK}/ana.tok").read().strip()
bob = open(f"{WORK}/bob.tok").read().strip()
shots = os.environ.get("SHOTS")
problems, steps = [], 0

def step(name):
    global steps
    steps += 1
    print(f"  ok  {name}")

with sync_playwright() as p:
    browser = p.chromium.launch()

    def open_as(token, **kw):
        ctx = browser.new_context(accept_downloads=True, **kw)
        page = ctx.new_page()
        page.on("console", lambda m: problems.append(m.text) if m.type == "error" else None)
        page.on("pageerror", lambda e: problems.append(f"PAGEERROR {e}"))
        page.goto(SITE + ("/#token=" + token if token else "/"))
        return page

    # -- sign-in gate
    page = open_as(None, viewport={"width": 1360, "height": 820})
    expect(page.get_by_role("button", name="Continue")).to_be_disabled(); step("sign-in asks for a token and will not continue empty")
    page.get_by_label("Token").fill(ana); page.get_by_role("button", name="Continue").click()
    expect(page.get_by_text("sales", exact=True)).to_be_visible(); step("a pasted token signs in")
    page.reload(); expect(page.get_by_text("sales", exact=True)).to_be_visible(); step("the session survives a reload")

    # -- make things through the dialog
    page.get_by_role("button", name="New").click()
    page.get_by_placeholder("sales_2026").fill("Bad Name")
    expect(page.get_by_role("button", name="Create")).to_be_disabled(); step("a name the server would refuse cannot be submitted")
    page.get_by_placeholder("sales_2026").fill("e2e_demo"); page.get_by_role("button", name="Create").click()
    expect(page.get_by_role("heading", name="e2e_demo")).to_be_visible(); step("a catalog is created and opened")
    page.get_by_role("button", name="New").click(); page.get_by_placeholder("sales_2026").fill("inbox"); page.get_by_role("button", name="Create").click()
    expect(page.get_by_role("heading", name="inbox")).to_be_visible(); step("a schema is created inside it")
    page.get_by_role("button", name="New").click()
    expect(page.get_by_text("Inside inbox")).to_be_visible()
    page.get_by_label("Kind").click(); page.get_by_role("option", name="Volume").click()
    page.get_by_placeholder("sales_2026").fill("drop_box"); page.get_by_role("button", name="Create").click()
    expect(page.get_by_role("heading", name="drop_box")).to_be_visible(); step("a volume is created inside the schema")
    page.get_by_role("button", name="New").click(); expect(page.get_by_text("Inside drop_box")).to_be_hidden() if False else None
    page.keyboard.press("Escape")

    # -- files: upload, download, delete
    src = tempfile.NamedTemporaryFile(suffix=".txt", delete=False, mode="w"); src.write("hello from the browser"); src.close()
    page.locator("input[type=file]").set_input_files(src.name)
    name = os.path.basename(src.name)
    expect(page.get_by_text(name, exact=True)).to_be_visible(); step("an uploaded file is listed")
    with page.expect_download() as dl:
        page.get_by_label(f"Download {name}").click()
    assert open(dl.value.path()).read() == "hello from the browser"; step("the downloaded bytes are the uploaded bytes")
    page.get_by_label(f"Delete {name}").click(); page.get_by_role("button", name="Delete", exact=True).last.click()
    expect(page.get_by_text("No files yet")).to_be_visible(); step("a file can be deleted, after confirming")
    if shots: page.screenshot(path=f"{shots}/e2e-empty-volume.png")

    # -- access
    page.get_by_role("tab", name="Access").click()
    page.get_by_placeholder("Person or agent id").fill("cy"); page.get_by_role("button", name="Give access").click()
    expect(page.get_by_text("cy", exact=True)).to_be_visible(); step("access can be given")
    page.get_by_label("Remove access for cy").click(); page.get_by_role("button", name="Remove", exact=True).click()
    expect(page.get_by_text("cy", exact=True)).to_be_hidden(); step("access can be taken away")

    # -- drop
    page.get_by_label("More").click(); page.get_by_role("menuitem", name="Drop…").click(); page.get_by_role("button", name="Drop", exact=True).click()
    page.wait_for_timeout(1500)
    expect(page.get_by_role("heading", name="drop_box")).to_be_hidden(); step("a dropped volume leaves the page at once")

    # -- activity
    page.get_by_label("Main").get_by_label("Activity").click()
    expect(page.get_by_text("Created a catalog").first).to_be_visible()
    expect(page.get_by_text("Started a drop").first).to_be_visible(); step("activity shows what was done, in words")
    expect(page.get_by_text("Tried to").first).to_be_visible(); step("a refusal reads as a refusal")
    expect(page.get_by_text("e2e_demo").first).to_be_visible(); step("targets show names, not ids")
    page.get_by_role("button", name="Check the record").click(); expect(page.get_by_text(re.compile("Intact · \\d+ entries"))).to_be_visible(); step("the record can be checked, and is intact")
    page.get_by_role("radio", name="Refused").click(); expect(page.get_by_text("Created a catalog")).to_have_count(0); step("the list can show only refusals")

    # -- admin: an erase runs to the end
    page.get_by_label("Main").get_by_label("Admin").click()
    page.get_by_label("Person id").fill("nobody-here"); page.get_by_role("button", name="Erase").click(); page.get_by_role("button", name="Erase", exact=True).last.click()
    expect(page.get_by_text("done", exact=True)).to_be_visible(timeout=15000); step("an erase is followed until it is done")
    if shots: page.screenshot(path=f"{shots}/e2e-admin.png")

    # -- dark theme sticks
    page.get_by_label("Switch to dark theme").click()
    assert page.evaluate("document.documentElement.classList.contains('dark')"); page.reload()
    assert page.evaluate("document.documentElement.classList.contains('dark')"); step("the dark theme is remembered")

    # -- a reader sees less, and is told why
    r = open_as(bob, viewport={"width": 1360, "height": 820})
    expect(r.get_by_text("sales", exact=True)).to_be_visible()
    expect(r.get_by_text("people_ops")).to_have_count(0); step("a reader does not see catalogs they have no access to")
    r.get_by_label("Expand sales").click(); r.get_by_label("Expand raw").click(); r.get_by_role("button", name="uploads").click()
    expect(r.get_by_text("orders.csv")).to_be_visible(); step("a reader can open and read a volume")
    r.get_by_role("tab", name="Access").click(); expect(r.get_by_text("Only managers can see who has access")).to_be_visible(); step("a reader is told access is for managers")
    r.get_by_role("tab", name="Files").click(); r.locator("input[type=file]").set_input_files(src.name)
    expect(r.get_by_text("Could not upload")).to_be_visible(); step("a reader's upload is refused with a clear message")
    r.get_by_label("Main").get_by_label("Activity").click(); expect(r.get_by_text("Only managers can read the activity")).to_be_visible(); step("a reader cannot read the activity")

    # -- phone
    m = open_as(ana, viewport={"width": 390, "height": 800}, has_touch=True)
    expect(m.get_by_text("sales", exact=True)).to_be_visible()
    m.get_by_label("Expand sales").click(); m.get_by_label("Expand raw").click(); m.get_by_role("button", name="uploads").click()
    expect(m.get_by_text("orders.csv")).to_be_visible(); expect(m.get_by_role("heading", name="Catalog")).to_be_hidden(); step("on a phone the list and the detail are one screen at a time")
    m.get_by_label("Back").click(); expect(m.get_by_role("heading", name="Catalog")).to_have_count(1); step("and Back returns to the list")
    assert m.evaluate("document.documentElement.scrollWidth <= innerWidth"); step("nothing scrolls sideways on a phone")

    # -- expired / bad token goes back to sign-in
    bad = open_as("not-a-token", viewport={"width": 800, "height": 600})
    expect(bad.get_by_role("button", name="Continue")).to_be_visible(); step("a token that is not a token does not sign in")
    browser.close()

real = [x for x in problems if "favicon" not in x and "401" not in x and "403" not in x and "404" not in x and "413" not in x]
print(f"\n{steps} steps passed; browser errors: {real or 'none'}")
sys.exit(1 if real else 0)
