import { profileSql } from "@/components/TableTab";
import { IDLE_STOP_MS, computeState, sizeLabel } from "./computeState";
import { describe, expect, it } from "vitest";
import { readError } from "./api";
import { expiryOf } from "./auth";
import { bytes, cellText, describe as say, parseTarget, shortTarget, toCsv, when } from "./format";
import { titleOf } from "./sqlTabs";
import { childKinds, shortId, validName } from "./kinds";
import { hrefOf, parseRoute } from "./route";
import { relayTarget, siteOf } from "./site";

const id = "c480ec64-2a04-4f53-8b8d-e7daffddbed9";

const WS = "6f1c1a52-0000-4000-8000-000000000001";

describe("sites", () => {
  it("tells a workspace's own site from the account site", () => {
    expect(siteOf("localhost:5180")).toEqual({ kind: "account", base: "localhost:5180" });
    expect(siteOf(`w-${WS}.localhost:5180`)).toEqual({ kind: "workspace", ws: WS, base: "localhost:5180" });
    expect(siteOf(`w-${WS}.data.loamz.com`)).toEqual({ kind: "workspace", ws: WS, base: "data.loamz.com" });
    // Anything that only looks like one is the account site, never a workspace.
    for (const h of ["w-nope.data.loamz.com", `x-${WS}.data.loamz.com`, `w-${WS}`, "data.loamz.com"]) {
      expect(siteOf(h).kind).toBe("account");
    }
  });

  const state = (origin: string) => `nonce.${btoa(origin).replace(/\+/g, "-").replace(/\//g, "_")}`;
  it("hands a sign-in code only to a workspace site of this account site", () => {
    const own = `http://w-${WS}.localhost:5180`;
    expect(relayTarget(state(own), "http:", "localhost:5180")).toBe(own);
  });
  it("refuses everywhere else", () => {
    const base = "data.loamz.com";
    for (const origin of [
      `https://w-${WS}.evil.com`, //                  another domain
      `https://w-${WS}.${base}.evil.com`, //          a lookalike
      `https://${base}`, //                           the account site itself
      `http://w-${WS}.${base}`, //                    the wrong protocol
      `https://w-${WS}.${base}/steal`, //             a path (not an origin)
      `https://user@w-${WS}.${base}`, //              credentials in the address
    ]) {
      expect(relayTarget(state(origin), "https:", base), origin).toBeNull();
    }
    for (const bad of ["", "nodot", "a.", "a.!!!", `a.${btoa("not a url")}`]) {
      expect(relayTarget(bad, "https:", base), bad).toBeNull();
    }
  });
});

describe("routes", () => {
  it("reads the account site's addresses", () => {
    expect(parseRoute("/", "account")).toEqual({ screen: "home" });
    expect(parseRoute("/account/acme/activity", "account")).toEqual({ screen: "account", org: "acme", tab: "activity" });
    expect(parseRoute("/account/acme", "account")).toEqual({ screen: "account", org: "acme", tab: "workspaces" });
    expect(parseRoute("/account/personal%3Aabc/admins", "account")).toEqual({ screen: "account", org: "personal:abc", tab: "admins" });
    expect(parseRoute("/account/acme/nonsense", "account")).toEqual({ screen: "account", org: "acme", tab: "workspaces" });
    expect(parseRoute("/catalog", "account")).toEqual({ screen: "home" });
  });
  it("reads a workspace site's addresses", () => {
    expect(parseRoute("/", "workspace", WS)).toEqual({ screen: "workspace", ws: WS });
    expect(parseRoute("/catalog", "workspace", WS)).toEqual({ screen: "catalog", ws: WS, id: null });
    expect(parseRoute(`/catalog/${id}`, "workspace", WS)).toEqual({ screen: "catalog", ws: WS, id });
    expect(parseRoute("/catalog/<script>", "workspace", WS)).toEqual({ screen: "catalog", ws: WS, id: null });
    expect(parseRoute("/settings", "workspace", WS)).toEqual({ screen: "settings", ws: WS });
    expect(parseRoute("/sql", "workspace", WS)).toEqual({ screen: "sql", ws: WS });
    expect(parseRoute("/compute", "workspace", WS)).toEqual({ screen: "compute", ws: WS });
    expect(parseRoute("/account/acme/compute", "account")).toEqual({ screen: "account", org: "acme", tab: "compute" });
    expect(parseRoute("/account/acme", "workspace", WS)).toEqual({ screen: "workspace", ws: WS }); // a workspace site has no account console
  });
  it("writes the address it reads", () => {
    for (const [r, kind] of [
      [{ screen: "home" }, "account"],
      [{ screen: "account", org: "personal:abc", tab: "erase" }, "account"],
      [{ screen: "workspace", ws: WS }, "workspace"],
      [{ screen: "catalog", ws: WS, id: null }, "workspace"],
      [{ screen: "catalog", ws: WS, id }, "workspace"],
      [{ screen: "settings", ws: WS }, "workspace"],
      [{ screen: "sql", ws: WS }, "workspace"],
      [{ screen: "compute", ws: WS }, "workspace"],
      [{ screen: "account", org: "acme", tab: "compute" }, "account"],
    ] as const) {
      expect(parseRoute(hrefOf(r), kind, WS)).toEqual(r);
    }
  });
});

describe("tokens", () => {
  const jwt = (claims: object) => `h.${btoa(JSON.stringify(claims)).replace(/=+$/, "")}.s`;
  it("reads the expiry", () => expect(expiryOf(jwt({ exp: 1700000000 }))).toBe(1700000000_000));
  it("treats anything unreadable as already expired", () => {
    expect(expiryOf("garbage")).toBe(0);
    expect(expiryOf(jwt({ sub: "x" }))).toBe(0);
    expect(expiryOf("")).toBe(0);
  });
});

describe("errors", () => {
  it("uses the server's own words", async () => {
    const e = await readError(new Response(JSON.stringify({ detail: "sales already exists here" }), { status: 409 }));
    expect([e.status, e.message]).toEqual([409, "sales already exists here"]);
  });
  it("never shows an internal message for a server fault", async () => {
    const e = await readError(new Response("<html>boom</html>", { status: 502 }));
    expect(e.message).toBe("Something went wrong on our side.");
  });
});

describe("names and kinds", () => {
  it("follows the server's rule for names", () => {
    for (const ok of ["sales", "sales_2026", "a"]) expect(validName(ok)).toBe(true);
    for (const bad of ["", "Sales", "1a", "a-b", "a b", "../x", "a".repeat(64)]) expect(validName(bad)).toBe(false);
  });
  it("offers only what can be made inside a thing", () => {
    expect(childKinds(null)).toEqual(["catalog"]);
    expect(childKinds("catalog")).toEqual(["schema"]);
    expect(childKinds("schema")).toEqual(["volume", "table", "view"]);
    expect(childKinds("volume")).toEqual([]);
  });
  it("shortens ids but leaves names alone", () => {
    expect(shortId(id)).toBe("c480ec64");
    expect(shortId("ana")).toBe("ana");
    expect(shortId("agent-helper")).toBe("agent-helper");
  });
});

describe("words", () => {
  it("sizes and times", () => {
    expect(bytes(0)).toBe("0 B");
    expect(bytes(1536)).toBe("1.5 KB");
    expect(bytes(48000)).toBe("47 KB");
    expect(when(1000, 1000 + 20_000)).toBe("just now");
    expect(when(0, 5 * 60_000)).toBe("5 min ago");
  });
  it("names a refusal by what it tried", () => {
    expect(say("write", "denied")).toBe("Tried to write to");
    expect(say("write", "ok")).toBe("write");
    expect(say("create.volume")).toBe("Created a volume");
    expect(say("grant.reader")).toBe("Gave reader access");
  });
  it("shows a name for a target when one is known", () => {
    expect(parseTarget(`volume ${id}/a/b.csv`)).toMatchObject({ kind: "volume", id, path: "/a/b.csv" });
    expect(shortTarget(`volume ${id}/a/b.csv`, { [id]: "uploads" })).toBe("volume uploads/a/b.csv");
    expect(shortTarget(`volume ${id}`)).toBe("volume c480ec64…");
    expect(shortTarget("tenant")).toBe("tenant");
  });
});

describe("results", () => {
  it("writes CSV a spreadsheet reads as data, never as a formula", () => {
    const cols = [{ name: "name" }, { name: "note" }];
    const csv = toCsv(cols, [["Ana", 'said "hi", twice'], ["=HYPERLINK(\"http://evil\")", "+1"], ["-2", "@SUM(A1)"], [42, null], ["line\nbreak", { a: 1 }]]);
    const rows = csv.split("\r\n");
    expect(rows[0]).toBe("name,note");
    expect(rows[1]).toBe('Ana,"said ""hi"", twice"');
    expect(rows[2]).toBe(`"'=HYPERLINK(""http://evil"")",'+1`);
    expect(rows[3]).toBe("'-2,'@SUM(A1)");
    expect(rows[4]).toBe("42,"); // a number is a number, and null is empty
    expect(csv).toContain('"{""a"":1}"');
  });
  it("shows cells as text", () => {
    expect([cellText(null), cellText(undefined), cellText(5), cellText(true), cellText([1, 2])]).toEqual(["", "", "5", "true", "[1,2]"]);
  });
  it("names a tab from the first words of its statement", () => {
    expect(titleOf("SELECT * FROM a.b.c", "Query 1")).toBe("SELECT * FROM a.b.c");
    expect(titleOf("-- note\nSELECT   count(*)\nFROM   shop.main.orders WHERE x", "Query 1")).toBe("SELECT count(*) FROM sho…");
    expect(titleOf("  \n ", "Query 1")).toBe("Query 1");
  });
});

describe("profileSql", () => {
  it("profiles each plain column in one statement and quotes names", () => {
    const sql = profileSql("shop.main.sales4", [
      { id: 1, name: "id", type: "long", required: false },
      { id: 2, name: 'we"ird', type: "string", required: false },
      { id: 3, name: "blob", type: "binary", required: false },
    ]);
    expect(sql.startsWith("SELECT count(*) AS total, count(\"id\") AS n0")).toBe(true);
    expect(sql).toContain('count("we""ird") AS n1');
    expect(sql).not.toContain("blob");
    expect(sql.endsWith("FROM shop.main.sales4")).toBe(true);
  });
});

describe("computeState", () => {
  const now = 1_000_000;
  it("keeps a serverless warehouse running for a minute after its last statement, then stopped", () => {
    const base = { kind: "serverless" as const, online: true };
    expect(computeState({ ...base, last_used_ms: now - 5_000 }, now)).toBe("running");
    expect(computeState({ ...base, last_used_ms: now - IDLE_STOP_MS - 1 }, now)).toBe("stopped");
    expect(computeState({ ...base, last_used_ms: 0 }, now)).toBe("stopped");
  });
  it("shows a machine as running only while its agent answers", () => {
    expect(computeState({ kind: "agent", online: true, last_used_ms: 0 }, now)).toBe("running");
    expect(computeState({ kind: "agent", online: false, last_used_ms: now }, now)).toBe("offline");
  });
  it("words the size", () => {
    expect(sizeLabel({ max_cpu_millis: 1000, max_memory_mb: 2048 })).toBe("1 CPU · 2 GB");
    expect(sizeLabel({ max_cpu_millis: 4000, max_memory_mb: 1536 })).toBe("4 CPUs · 1.5 GB");
  });
});
