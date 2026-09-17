import { Clock, Effect } from "effect";

export const TEST_WEBHOOK_SECRET = "test-webhook-secret";

export const VALID_PAYLOAD = {
  event: "messages",
  space: {
    id: "test-space",
    platform: "iMessage",
    type: "dm",
    phone: "private-line",
  },
  message: {
    id: "test-message",
    platform: "iMessage",
    direction: "inbound",
    timestamp: "2026-05-14T19:06:32.000Z",
    sender: { id: "private-sender", platform: "iMessage" },
    space: {
      id: "test-space",
      platform: "iMessage",
      type: "dm",
      phone: "private-line",
    },
    content: { type: "text", text: "private-message" },
  },
} as const;

export const signWebhookBytes = async (
  bodyBytes: Uint8Array,
  timestamp?: string,
): Promise<{ readonly signature: string; readonly timestamp: string }> => {
  const resolvedTimestamp =
    timestamp ??
    String(
      Math.floor((await Effect.runPromise(Clock.currentTimeMillis)) / 1000),
    );

  const encoder = new TextEncoder();
  const prefix = encoder.encode(`v0:${resolvedTimestamp}:`);
  const signedBytes = new Uint8Array(prefix.length + bodyBytes.length);

  signedBytes.set(prefix);
  signedBytes.set(bodyBytes, prefix.length);

  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(TEST_WEBHOOK_SECRET),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );

  const bytes = new Uint8Array(
    await crypto.subtle.sign("HMAC", key, signedBytes),
  );

  const signature = `v0=${Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("")}`;

  return { signature, timestamp: resolvedTimestamp };
};

export const signWebhook = (body: string, timestamp?: string) =>
  signWebhookBytes(new TextEncoder().encode(body), timestamp);

export const signedWebhookHeaders = async (
  body: string,
): Promise<Readonly<Record<string, string>>> => {
  const { signature, timestamp } = await signWebhook(body);

  return {
    "content-type": "application/json",
    "x-spectrum-event": "messages",
    "x-spectrum-signature": signature,
    "x-spectrum-timestamp": timestamp,
    "x-spectrum-webhook-id": "test-webhook",
  };
};
