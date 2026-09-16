import { Clock, Effect } from "effect";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import worker from "../src/worker";

const TEST_WEBHOOK_SECRET = "test-webhook-secret";

const VALID_BODY = JSON.stringify({
  event: "messages",
  space: { id: "test-space", type: "dm", phone: "shared" },
  message: {
    id: "test-message",
    platform: "iMessage",
    direction: "inbound",
    sender: { id: "test-sender" },
    content: { type: "text", text: "test-message-body" },
  },
});

const sign = async (timestamp: string, body: string): Promise<string> => {
  const encoder = new TextEncoder();

  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(TEST_WEBHOOK_SECRET),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );

  const bytes = new Uint8Array(
    await crypto.subtle.sign(
      "HMAC",
      key,
      encoder.encode(`v0:${timestamp}:${body}`),
    ),
  );

  return `v0=${Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("")}`;
};

/**
 * Implement these behavioral tests in order. Exercise the Web handler with a
 * Request rather than calling private helpers. Keep fixtures synthetic: never
 * copy real secrets, signatures, phone numbers, or message bodies into tests.
 */
describe("trusted Photon webhook ingress", () => {
  beforeEach(() => {
    vi.stubEnv("WEBHOOK_SECRET", TEST_WEBHOOK_SECRET);
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("accepts a valid signed Photon text message", async () => {
    const nowMillis = await Effect.runPromise(Clock.currentTimeMillis);
    const timestamp = String(Math.floor(nowMillis / 1000));
    const signature = await sign(timestamp, VALID_BODY);

    const response = await worker.fetch(
      new Request("https://satchel.test/webhooks/photon", {
        method: "POST",
        headers: {
          "x-spectrum-signature": signature,
          "x-spectrum-timestamp": timestamp,
          "x-spectrum-webhook-id": "test-webhook",
        },
        body: VALID_BODY,
      }),
    );

    expect(response.status).toBe(200);
    expect(await response.text()).toBe("ok");
  });

  it("rejects a request with missing authentication headers", async () => {
    const response = await worker.fetch(
      new Request("https://satchel.test/webhooks/photon", { method: "POST" }),
    );

    expect(response.status).toBe(400);
  });

  it("rejects a malformed timestamp", async () => {
    const response = await worker.fetch(
      new Request("https://satchel.test/webhooks/photon", {
        method: "POST",
        headers: {
          "x-spectrum-signature": "v0=placeholder",
          "x-spectrum-timestamp": "not-a-timestamp",
          "x-spectrum-webhook-id": "test-webhook",
        },
      }),
    );

    expect(response.status).toBe(400);
  });

  it("rejects a stale timestamp", async () => {
    const response = await worker.fetch(
      new Request("https://satchel.test/webhooks/photon", {
        method: "POST",
        headers: {
          "x-spectrum-signature": "v0=placeholder",
          "x-spectrum-timestamp": "0",
          "x-spectrum-webhook-id": "test-webhook",
        },
      }),
    );

    expect(response.status).toBe(400);
  });

  it("rejects an invalid signature before decoding JSON", async () => {
    const nowMillis = await Effect.runPromise(Clock.currentTimeMillis);
    const timestamp = String(Math.floor(nowMillis / 1000));

    const response = await worker.fetch(
      new Request("https://satchel.test/webhooks/photon", {
        method: "POST",
        headers: {
          "x-spectrum-signature": `v0=${"0".repeat(64)}`,
          "x-spectrum-timestamp": timestamp,
          "x-spectrum-webhook-id": "test-webhook",
        },
        body: "not-json",
      }),
    );

    expect(response.status).toBe(401);
  });

  it("rejects malformed authenticated JSON without crashing", async () => {
    const nowMillis = await Effect.runPromise(Clock.currentTimeMillis);
    const timestamp = String(Math.floor(nowMillis / 1000));
    const body = "{";
    const signature = await sign(timestamp, body);

    const response = await worker.fetch(
      new Request("https://satchel.test/webhooks/photon", {
        method: "POST",
        headers: {
          "x-spectrum-signature": signature,
          "x-spectrum-timestamp": timestamp,
          "x-spectrum-webhook-id": "test-webhook",
        },
        body,
      }),
    );

    expect(response.status).toBe(400);
  });

  it.todo("ignores outbound, non-text, group, and senderless events");
  it.todo("produces a provider-neutral verified inbound message");
  it.todo("keeps secrets, phone numbers, and message text out of logs");
});
