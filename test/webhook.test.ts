import { env, runInDurableObject, SELF } from "cloudflare:test";
import { HttpServerRequest } from "@effect/platform";
import * as spectrumWebhook from "@spectrum-ts/core/webhook";
import {
  Clock,
  ConfigProvider,
  Effect,
  Option,
  Schema,
  TestClock,
  TestContext,
} from "effect";
import { describe, expect, it, vi } from "vitest";
import { Hmac } from "../src/hmac";
import worker from "../src/worker";

const TEST_WEBHOOK_SECRET = "test-webhook-secret";

const TEST_WEBHOOK_NOW = Date.parse("2026-05-14T19:06:32.000Z");

const TEST_WEBHOOK_TIMESTAMP = String(Math.floor(TEST_WEBHOOK_NOW / 1000));

const VALID_PAYLOAD = {
  event: "messages",
  space: {
    id: "test-space",
    platform: "imessage",
    type: "dm",
    phone: "private-line",
  },
  message: {
    id: "test-message",
    platform: "imessage",
    direction: "inbound",
    timestamp: "2026-05-14T19:06:32.000Z",
    sender: { id: "private-sender", platform: "imessage" },
    space: {
      id: "test-space",
      platform: "imessage",
      type: "dm",
      phone: "private-line",
    },
    content: { type: "text", text: "private-message" },
  },
} as const;

const signWebhookBytes = async (
  bodyBytes: Uint8Array,
  timestamp: string,
): Promise<string> => {
  const encoder = new TextEncoder();
  const prefix = encoder.encode(`v0:${timestamp}:`);
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

  return `v0=${Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("")}`;
};

const signWebhook = (body: string, timestamp: string) =>
  signWebhookBytes(new TextEncoder().encode(body), timestamp);

/** Explicit live-clock boundary for real Worker runtime tests. */
const currentWebhookTimestamp = () =>
  Effect.runPromise(
    Clock.currentTimeMillis.pipe(
      Effect.map((now) => String(Math.floor(now / 1000))),
    ),
  );

const signedWebhookHeaders = async (
  body: string,
  timestamp: string,
): Promise<Readonly<Record<string, string>>> => {
  const signature = await signWebhook(body, timestamp);

  return {
    "content-type": "application/json",
    "x-spectrum-event": "messages",
    "x-spectrum-signature": signature,
    "x-spectrum-timestamp": timestamp,
    "x-spectrum-webhook-id": "test-webhook",
  };
};

const WEBHOOK_URL = "https://satchel.test/webhooks/photon";

const testConfig = ConfigProvider.fromMap(
  new Map([["WEBHOOK_SECRET", TEST_WEBHOOK_SECRET]]),
);

const accountTestConfig = ConfigProvider.fromMap(
  new Map([["ACCOUNT_ID_SECRET", "test-account-id-secret"]]),
);

const encodeJson = Schema.encodeSync(Schema.parseJson());

const signedRequest = async (
  body: string,
  timestamp: string,
  additionalHeaders: Readonly<Record<string, string>> = {},
) =>
  new Request(WEBHOOK_URL, {
    method: "POST",
    headers: {
      ...(await signedWebhookHeaders(body, timestamp)),
      ...additionalHeaders,
    },
    body,
  });

const liveSignedRequest = async (
  body: string,
  additionalHeaders: Readonly<Record<string, string>> = {},
) => signedRequest(body, await currentWebhookTimestamp(), additionalHeaders);

const signedByteRequest = async (body: Uint8Array) => {
  const timestamp = await currentWebhookTimestamp();
  const signature = await signWebhookBytes(body, timestamp);

  return new Request(WEBHOOK_URL, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-spectrum-event": "messages",
      "x-spectrum-signature": signature,
      "x-spectrum-timestamp": timestamp,
      "x-spectrum-webhook-id": "test-webhook",
    },
    body,
  });
};

const fetchWorker = (request: Request) => worker.fetch(request, env);

const verify = (rawBody: string) =>
  Effect.gen(function* () {
    yield* TestClock.setTime(TEST_WEBHOOK_NOW);

    const request = HttpServerRequest.fromWeb(
      yield* Effect.promise(() =>
        signedRequest(rawBody, TEST_WEBHOOK_TIMESTAMP),
      ),
    );

    return yield* Hmac.verifyWebhook().pipe(
      Effect.provideService(HttpServerRequest.HttpServerRequest, request),
    );
  }).pipe(Effect.provide(Hmac.Default), Effect.withConfigProvider(testConfig));

const itEffect = <E>(name: string, test: () => Effect.Effect<void, E>) =>
  it(name, () =>
    Effect.runPromise(test().pipe(Effect.provide(TestContext.TestContext))),
  );

describe("Photon webhook contract", () => {
  it("rejects malformed or replay-prone authentication metadata", async () => {
    const cases = [
      {
        name: "missing authentication headers",
        request: () => new Request(WEBHOOK_URL, { method: "POST" }),
        status: 400,
        body: "missing or malformed headers",
      },
      {
        name: "malformed timestamp",
        request: () =>
          new Request(WEBHOOK_URL, {
            method: "POST",
            headers: {
              "x-spectrum-event": "messages",
              "x-spectrum-signature": "v0=placeholder",
              "x-spectrum-timestamp": "not-a-timestamp",
              "x-spectrum-webhook-id": "test-webhook",
            },
          }),
        status: 400,
        body: "invalid timestamp",
      },
      {
        name: "stale timestamp",
        request: () =>
          new Request(WEBHOOK_URL, {
            method: "POST",
            headers: {
              "x-spectrum-event": "messages",
              "x-spectrum-signature": "v0=placeholder",
              "x-spectrum-timestamp": "0",
              "x-spectrum-webhook-id": "test-webhook",
            },
          }),
        status: 400,
        body: "stale timestamp",
      },
    ];

    for (const { name, request, status, body } of cases) {
      const response = await fetchWorker(request());

      expect(response.status, name).toBe(status);
      expect(await response.text(), name).toBe(body);
    }
  });

  it("authenticates exact body bytes before decoding JSON", async () => {
    const invalidJson = "{";

    const tamperedResponse = await fetchWorker(
      await liveSignedRequest(invalidJson, {
        "x-spectrum-signature": `v0=${"0".repeat(64)}`,
      }),
    );

    expect(tamperedResponse.status).toBe(401);
    expect(await tamperedResponse.text()).toBe("invalid signature");

    const authenticatedResponse = await fetchWorker(
      await liveSignedRequest(invalidJson),
    );

    expect(authenticatedResponse.status).toBe(400);
    expect(await authenticatedResponse.text()).toBe("invalid webhook body");

    const malformedUtf8 = Uint8Array.of(0xff);

    const byteResponse = await fetchWorker(
      await signedByteRequest(malformedUtf8),
    );

    expect(byteResponse.status).toBe(400);
    expect(await byteResponse.text()).toBe("invalid webhook body");
  });

  it("maps verifier failures to stable HTTP responses", async () => {
    const verifier = vi.spyOn(spectrumWebhook, "verifySpectrumSignature");

    const cases = [
      {
        result: { ok: false, reason: "expired" } as const,
        status: 400,
        body: "stale timestamp",
      },
      {
        result: { ok: false, reason: "missing-headers" } as const,
        status: 400,
        body: "invalid timestamp",
      },
    ];

    for (const testCase of cases) {
      verifier.mockResolvedValueOnce(testCase.result);

      const response = await fetchWorker(
        await liveSignedRequest(encodeJson(VALID_PAYLOAD)),
      );

      expect(response.status).toBe(testCase.status);
      expect(await response.text()).toBe(testCase.body);
    }

    for (const rejection of [
      new TypeError("private verifier details"),
      "private verifier details",
    ]) {
      verifier.mockRejectedValueOnce(rejection);

      const response = await fetchWorker(
        await liveSignedRequest(encodeJson(VALID_PAYLOAD)),
      );

      expect(response.status).toBe(500);
      expect(await response.text()).toBe("signature verification failed");
    }

    verifier.mockRestore();
  });

  it("acknowledges unsupported event signals for forward compatibility", async () => {
    const requests = [
      await liveSignedRequest(encodeJson(VALID_PAYLOAD), {
        "x-spectrum-event": "future-event",
      }),
      await liveSignedRequest(
        encodeJson({ event: "future-event", data: { id: "test-event" } }),
      ),
    ];

    for (const request of requests) {
      const response = await fetchWorker(request);

      expect(response.status).toBe(200);
      expect(await response.text()).toBe("ignored");
    }
  });

  it("fails deliberately when the Worker secret binding is missing", async () => {
    const response = await worker.fetch(
      await liveSignedRequest(encodeJson(VALID_PAYLOAD)),
      { ACCOUNTS: env.ACCOUNTS },
    );

    expect(response.status).toBe(500);
    expect(await response.text()).toBe("webhook is not configured");
  });

  it("fails deliberately when the account identity secret is missing", async () => {
    const response = await worker.fetch(
      await liveSignedRequest(encodeJson(VALID_PAYLOAD)),
      {
        ACCOUNTS: env.ACCOUNTS,
        WEBHOOK_SECRET: TEST_WEBHOOK_SECRET,
      },
    );

    expect(response.status).toBe(500);
    expect(await response.text()).toBe("message acceptance failed");
  });

  it("maps account storage failure to a retryable response", async () => {
    const payload = {
      ...VALID_PAYLOAD,
      message: {
        ...VALID_PAYLOAD.message,
        sender: {
          ...VALID_PAYLOAD.message.sender,
          id: "storage-failure-sender",
        },
      },
    };

    const accountId = await Effect.runPromise(
      Hmac.deriveAccountId("imessage", payload.message.sender.id).pipe(
        Effect.provide(Hmac.Default),
        Effect.withConfigProvider(accountTestConfig),
      ),
    );

    const account = env.ACCOUNTS.getByName(accountId);

    await runInDurableObject(account, (_instance, state) => {
      state.storage.sql.exec("DROP TABLE inbox");
    });

    const response = await fetchWorker(
      await liveSignedRequest(encodeJson(payload)),
    );

    expect(response.status).toBe(500);
    expect(await response.text()).toBe("message acceptance failed");
  });

  itEffect("returns the complete verified inbound message", () =>
    Effect.gen(function* () {
      const verified = yield* verify(encodeJson(VALID_PAYLOAD));

      const expected = {
        deliveryId: "test-webhook:test-message",
        messageId: "test-message",
        platform: "imessage",
        senderId: "private-sender",
        spaceId: "test-space",
        text: "private-message",
      };

      expect(verified).toEqual(
        Option.some({
          ...expected,
          servingLine: "private-line",
        }),
      );

      const withoutServingLine = yield* verify(
        encodeJson({
          ...VALID_PAYLOAD,
          space: { id: "test-space", platform: "imessage", type: "dm" },
        }),
      );

      expect(withoutServingLine).toEqual(Option.some(expected));
    }),
  );

  itEffect("filters messages outside the trusted inbound text contract", () =>
    Effect.gen(function* () {
      const cases = [
        {
          name: "non-iMessage platform",
          payload: {
            ...VALID_PAYLOAD,
            message: { ...VALID_PAYLOAD.message, platform: "WhatsApp" },
          },
        },
        {
          name: "outbound message",
          payload: {
            ...VALID_PAYLOAD,
            message: { ...VALID_PAYLOAD.message, direction: "outbound" },
          },
        },
        {
          name: "group message",
          payload: {
            ...VALID_PAYLOAD,
            space: { ...VALID_PAYLOAD.space, type: "group" },
          },
        },
        {
          name: "senderless message",
          payload: {
            ...VALID_PAYLOAD,
            message: { ...VALID_PAYLOAD.message, sender: undefined },
          },
        },
        {
          name: "non-text message",
          payload: {
            ...VALID_PAYLOAD,
            message: {
              ...VALID_PAYLOAD.message,
              content: { type: "attachment", name: "test.jpg" },
            },
          },
        },
      ];

      for (const { name, payload } of cases) {
        const verified = yield* verify(encodeJson(payload));

        expect(verified, name).toEqual(Option.none());
      }
    }),
  );
});

describe("Cloudflare Worker runtime", () => {
  it("accepts a signed webhook in workerd without logging sensitive fields", async () => {
    const log = vi.spyOn(console, "log").mockImplementation(() => undefined);
    const body = JSON.stringify(VALID_PAYLOAD);
    const timestamp = await currentWebhookTimestamp();
    const headers = await signedWebhookHeaders(body, timestamp);

    const response = await SELF.fetch(
      new Request(WEBHOOK_URL, {
        method: "POST",
        headers,
        body,
      }),
    );

    expect(response.status).toBe(200);
    expect(await response.text()).toBe("ok");

    const logs = JSON.stringify(log.mock.calls);

    log.mockRestore();

    expect(logs).toContain("Durably accepted inbound message");
    expect(logs).toContain("deliveryId=test-webhook:test-message");
    expect(logs).not.toContain(TEST_WEBHOOK_SECRET);
    expect(logs).not.toContain(VALID_PAYLOAD.space.phone);
    expect(logs).not.toContain(VALID_PAYLOAD.message.sender.id);
    expect(logs).not.toContain(VALID_PAYLOAD.message.content.text);
  });

  it("enforces the webhook body limit in workerd", async () => {
    const body = "x".repeat(1024 * 1024 + 1);
    const timestamp = await currentWebhookTimestamp();

    const response = await SELF.fetch(
      new Request(WEBHOOK_URL, {
        method: "POST",
        headers: {
          "x-spectrum-event": "messages",
          "x-spectrum-signature": `v0=${"0".repeat(64)}`,
          "x-spectrum-timestamp": timestamp,
          "x-spectrum-webhook-id": "test-webhook",
        },
        body,
      }),
    );

    expect(response.status).toBe(413);
    expect(await response.text()).toBe("webhook body too large");
  });
});
