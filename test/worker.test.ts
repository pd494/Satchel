import { describe, expect, it } from "vitest";
import worker from "../src/worker";

describe("Cloudflare Worker", () => {
  it("serves the health route", async () => {
    const response = await worker.fetch(new Request("https://satchel.test/"));

    expect(response.status).toBe(200);
    expect(await response.text()).toBe("satchel.ok");
  });
});
