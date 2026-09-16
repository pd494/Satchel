import { Clock, Effect } from "effect";

export const TEST_WEBHOOK_SECRET = "test-webhook-secret";

export const VALID_PAYLOAD = {
  event: "messages",
  space: { id: "test-space", type: "dm", phone: "private-line" },
  message: {
    id: "test-message",
    platform: "iMessage",
    direction: "inbound",
    sender: { id: "private-sender" },
    content: { type: "text", text: "private-message" },
  },
} as const;

export const signWebhook = async (
  body: string,
  timestamp?: string,
): Promise<{ readonly signature: string; readonly timestamp: string }> => {
  const resolvedTimestamp =
    timestamp ??
    String(
      Math.floor((await Effect.runPromise(Clock.currentTimeMillis)) / 1000),
    );

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
      encoder.encode(`v0:${resolvedTimestamp}:${body}`),
    ),
  );

  const signature = `v0=${Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("")}`;

  return { signature, timestamp: resolvedTimestamp };
};

export const signedWebhookHeaders = async (
  body: string,
): Promise<Readonly<Record<string, string>>> => {
  const { signature, timestamp } = await signWebhook(body);

  return {
    "content-type": "application/json",
    "x-spectrum-signature": signature,
    "x-spectrum-timestamp": timestamp,
    "x-spectrum-webhook-id": "test-webhook",
  };
};
