import { HttpServerRequest } from "@effect/platform";
import { describe, expect, it } from "@effect/vitest";
import { ConfigProvider, Effect, Option, Schema } from "effect";
import { afterEach, beforeEach, vi } from "vitest";
import { verifyPhotonWebhook } from "../src/photon-webhook";
import worker from "../src/worker";
import {
  signedWebhookHeaders,
  TEST_WEBHOOK_SECRET,
  VALID_PAYLOAD,
} from "./photon-webhook-fixture";

const WEBHOOK_URL = "https://satchel.test/webhooks/photon";

const testConfig = ConfigProvider.fromMap(
  new Map([["WEBHOOK_SECRET", TEST_WEBHOOK_SECRET]]),
);

const encodeJson = Schema.encodeSync(Schema.parseJson());

const signedRequest = async (
  body: string,
  additionalHeaders: Readonly<Record<string, string>> = {},
) =>
  new Request(WEBHOOK_URL, {
    method: "POST",
    headers: { ...(await signedWebhookHeaders(body)), ...additionalHeaders },
    body,
  });

const verify = (rawBody: string) =>
  Effect.gen(function* () {
    const request = HttpServerRequest.fromWeb(
      yield* Effect.promise(() => signedRequest(rawBody)),
    );

    return yield* verifyPhotonWebhook().pipe(
      Effect.provideService(HttpServerRequest.HttpServerRequest, request),
    );
  }).pipe(Effect.withConfigProvider(testConfig));

describe("Photon webhook contract", () => {
  beforeEach(() => {
    vi.stubEnv("WEBHOOK_SECRET", TEST_WEBHOOK_SECRET);
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

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
      const response = await worker.fetch(request());

      expect(response.status, name).toBe(status);
      expect(await response.text(), name).toBe(body);
    }
  });

  it("authenticates exact body bytes before decoding JSON", async () => {
    const invalidJson = "{";

    const tamperedResponse = await worker.fetch(
      await signedRequest(invalidJson, {
        "x-spectrum-signature": `v0=${"0".repeat(64)}`,
      }),
    );

    expect(tamperedResponse.status).toBe(401);
    expect(await tamperedResponse.text()).toBe("invalid signature");

    const authenticatedResponse = await worker.fetch(
      await signedRequest(invalidJson),
    );

    expect(authenticatedResponse.status).toBe(400);
    expect(await authenticatedResponse.text()).toBe("invalid webhook body");
  });

  it("acknowledges unsupported event signals for forward compatibility", async () => {
    const requests = [
      await signedRequest(encodeJson(VALID_PAYLOAD), {
        "x-spectrum-event": "future-event",
      }),
      await signedRequest(
        encodeJson({ event: "future-event", data: { id: "test-event" } }),
      ),
    ];

    for (const request of requests) {
      const response = await worker.fetch(request);

      expect(response.status).toBe(200);
      expect(await response.text()).toBe("ignored");
    }
  });

  it.live("returns the complete verified inbound message", () =>
    Effect.gen(function* () {
      const verified = yield* verify(encodeJson(VALID_PAYLOAD));

      const expected = {
        deliveryId: "test-webhook:test-message",
        messageId: "test-message",
        platform: "iMessage",
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
          space: { id: "test-space", type: "dm" },
        }),
      );

      expect(withoutServingLine).toEqual(Option.some(expected));
    }),
  );

  it.live("filters messages outside the trusted inbound text contract", () =>
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
