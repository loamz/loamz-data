import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";

vi.mock("./auth", () => ({
  signOut: async () => undefined,
  token: async () => "test-token",
}));

describe("catalog search API", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("encodes filters and returns visible hits with paths", async () => {
    const fetch = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          items: [{ id: "one", kind: "table", name: "orders", path: "sales.raw.orders" }],
        }),
        { status: 200 },
      ),
    );
    vi.stubGlobal("fetch", fetch);
    const hits = await api.search("workspace/id", {
      q: "Finance 100%_",
      kind: "table",
      tag: "finance_x",
      owner: "user:ana",
      limit: 25,
    });

    expect(hits).toEqual([{ id: "one", kind: "table", name: "orders", path: "sales.raw.orders" }]);
    const [url, init] = fetch.mock.calls[0] as [string, RequestInit];
    expect(new URL(url, "http://localhost").pathname).toBe("/api/v1/workspaces/workspace%2Fid/search");
    const query = new URL(url, "http://localhost").searchParams;
    expect([...query.entries()]).toEqual([
      ["q", "Finance 100%_"],
      ["kind", "table"],
      ["tag", "finance_x"],
      ["owner", "user:ana"],
      ["limit", "25"],
    ]);
    expect(init.method).toBe("GET");
  });
});
