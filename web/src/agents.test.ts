import { describe, expect, it } from "vitest";
import { agentsUrl, chatRequest } from "./agents";

describe("the ask box", () => {
  it("is off when no agents address is configured, so Data works alone", () => {
    expect(agentsUrl).toBeNull();
  });
  it("sends one message with the workspace and the narrowed catalogs", () => {
    expect(chatRequest("ws-1", "how many trips?", ["nyc"], "t-1")).toEqual({
      thread_id: "t-1",
      messages: [{ role: "user", content: "how many trips?" }],
      data: { workspace: "ws-1", catalogs: ["nyc"] },
    });
    expect(chatRequest("ws-1", "q", [], "t").data.catalogs).toEqual([]);
  });
});
