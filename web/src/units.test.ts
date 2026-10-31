import { describe, expect, it } from "vitest";
import { readError } from "./api";
import { expiryOf } from "./auth";
import { bytes, describe as say, parseTarget, shortTarget, when } from "./format";
import { childKinds, shortId, validName } from "./kinds";
import { hrefOf, parseRoute } from "./route";

const id = "c480ec64-2a04-4f53-8b8d-e7daffddbed9";

describe("routes", () => {
  it("reads the address", () => {
    expect(parseRoute("")).toEqual({ screen: "catalog", id: null });
    expect(parseRoute("#/activity")).toEqual({ screen: "activity" });
    expect(parseRoute("#/admin")).toEqual({ screen: "admin" });
    expect(parseRoute(`#/catalog/${id}`)).toEqual({ screen: "catalog", id });
  });
  it("ignores an id that cannot be one", () => {
    expect(parseRoute("#/catalog/../etc")).toEqual({ screen: "catalog", id: null });
    expect(parseRoute("#/catalog/<script>")).toEqual({ screen: "catalog", id: null });
  });
  it("writes the address it reads", () => {
    for (const r of [{ screen: "catalog", id: null }, { screen: "catalog", id }, { screen: "activity" }, { screen: "admin" }] as const) {
      expect(parseRoute(hrefOf(r))).toEqual(r);
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
