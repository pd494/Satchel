import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { createTestHarness, type TestHarness } from "wrangler";
import {
  currentWebhookTimestamp,
  signedWebhookHeaders,
  TEST_WEBHOOK_SECRET,
  VALID_PAYLOAD,
} from "./photon-webhook-fixture";

describe("Cloudflare Worker runtime", () => {
  let server: TestHarness;

  beforeAll(async () => {
    vi.stubEnv("CLOUDFLARE_LOAD_DEV_VARS_FROM_DOT_ENV", "false");
    server = createTestHarness({
      root: process.cwd(),
      workers: [
        {
          configPath: "./wrangler.jsonc",
          secrets: { WEBHOOK_SECRET: TEST_WEBHOOK_SECRET },
        },
      ],
    });
    await server.listen();
  });

  afterAll(async () => {
    try {
      await server.close();
    } finally {
      vi.unstubAllEnvs();
    }
  });

  it("accepts a signed webhook in workerd without logging sensitive fields", async () => {
    server.clearLogs();
    const body = JSON.stringify(VALID_PAYLOAD);
    const timestamp = await currentWebhookTimestamp();
    const headers = await signedWebhookHeaders(body, timestamp);

    const response = await server.fetch("/webhooks/photon", {
      method: "POST",
      headers,
      body,
    });

    expect(response.status).toBe(200);
    expect(await response.text()).toBe("ok");

    const logs = JSON.stringify(server.getLogs());

    expect(logs).toContain("Accepted verified inbound message");
    expect(logs).toContain("deliveryId=test-webhook:test-message");
    expect(logs).not.toContain(TEST_WEBHOOK_SECRET);
    expect(logs).not.toContain(VALID_PAYLOAD.space.phone);
    expect(logs).not.toContain(VALID_PAYLOAD.message.sender.id);
    expect(logs).not.toContain(VALID_PAYLOAD.message.content.text);
  });

  it("enforces the webhook body limit in workerd", async () => {
    const body = "x".repeat(1024 * 1024 + 1);
    const timestamp = await currentWebhookTimestamp();

    const response = await server.fetch("/webhooks/photon", {
      method: "POST",
      headers: {
        "x-spectrum-event": "messages",
        "x-spectrum-signature": `v0=${"0".repeat(64)}`,
        "x-spectrum-timestamp": timestamp,
        "x-spectrum-webhook-id": "test-webhook",
      },
      body,
    });

    expect(response.status).toBe(413);
    expect(await response.text()).toBe("webhook body too large");
  });
});
