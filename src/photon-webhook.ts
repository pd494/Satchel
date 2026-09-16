import { HttpServerRequest, HttpServerResponse } from "@effect/platform";
import {
  Chunk,
  Clock,
  Config,
  Effect,
  Option,
  Redacted,
  Schema,
  Stream,
} from "effect";
import {
  InvalidWebhookBodyError,
  InvalidWebhookHeadersError,
  InvalidWebhookSignatureError,
  InvalidWebhookTimestampError,
  StaleWebhookTimestampError,
  WebhookBodyTooLargeError,
  WebhookCryptoError,
} from "./errors";

const TIMESTAMP_TOLERANCE_SECONDS = 5 * 60;

const MAX_WEBHOOK_BODY_BYTES = 1024 * 1024;

const SIGNATURE_PATTERN = /^v0=[0-9a-f]{64}$/;

const webhookSecretConfig = Config.redacted("WEBHOOK_SECRET");

const PhotonWebhookHeaders = Schema.Struct({
  "x-spectrum-event": Schema.optional(Schema.String.pipe(Schema.minLength(1))),
  "x-spectrum-signature": Schema.String.pipe(Schema.minLength(1)),
  "x-spectrum-timestamp": Schema.String.pipe(Schema.minLength(1)),
  "x-spectrum-webhook-id": Schema.String.pipe(Schema.minLength(1)),
});

const PhotonWebhookEnvelope = Schema.Struct({
  event: Schema.String,
});

const PhotonWebhookBody = Schema.Struct({
  event: Schema.Literal("messages"),
  space: Schema.Struct({
    id: Schema.String,
    type: Schema.optional(Schema.String),
    phone: Schema.optional(Schema.String),
  }),
  message: Schema.Struct({
    id: Schema.String,
    platform: Schema.String,
    direction: Schema.String,
    sender: Schema.optional(
      Schema.Struct({
        id: Schema.String,
      }),
    ),
    content: Schema.Struct({
      type: Schema.String,
      text: Schema.optional(Schema.String),
    }),
  }),
});

type PhotonWebhookBody = typeof PhotonWebhookBody.Type;

export interface VerifiedInboundMessage {
  readonly deliveryId: string;
  readonly messageId: string;
  readonly platform: string;
  readonly senderId: string;
  readonly spaceId: string;
  readonly servingLine?: string;
  readonly text: string;
}

const causeName = (cause: unknown): string =>
  cause instanceof Error ? cause.name : "Unknown rejection";

const decodeHex = (hex: string): Uint8Array<ArrayBuffer> => {
  const bytes = new Uint8Array(new ArrayBuffer(hex.length / 2));

  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(hex.slice(index * 2, index * 2 + 2), 16);
  }

  return bytes;
};

const validateTimestamp = Effect.fn("PhotonWebhook.validateTimestamp")(
  function* (timestamp: string) {
    const timestampSeconds = Number(timestamp);

    if (!Number.isSafeInteger(timestampSeconds))
      return yield* new InvalidWebhookTimestampError({
        message: "Webhook timestamp must be a Unix epoch second",
      });

    const nowMillis = yield* Clock.currentTimeMillis;

    const ageSeconds = Math.abs(
      Math.floor(nowMillis / 1000) - timestampSeconds,
    );

    if (ageSeconds > TIMESTAMP_TOLERANCE_SECONDS)
      return yield* new StaleWebhookTimestampError({
        message: "Webhook timestamp is outside the allowed window",
      });
  },
);

const verifySignature = Effect.fn("PhotonWebhook.verifySignature")(
  function* (input: {
    readonly rawBody: string;
    readonly secret: Redacted.Redacted<string>;
    readonly signature: string;
    readonly timestamp: string;
  }) {
    if (!SIGNATURE_PATTERN.test(input.signature))
      return yield* new InvalidWebhookSignatureError({
        message: "Webhook signature has an invalid format",
      });

    const encoder = new TextEncoder();
    const secretBytes = encoder.encode(Redacted.value(input.secret));

    const signedBytes = encoder.encode(
      `v0:${input.timestamp}:${input.rawBody}`,
    );

    const signatureBytes = decodeHex(input.signature.slice("v0=".length));

    const signatureIsValid = yield* Effect.tryPromise({
      try: async () => {
        const key = await crypto.subtle.importKey(
          "raw",
          secretBytes,
          { name: "HMAC", hash: "SHA-256" },
          false,
          ["verify"],
        );

        return crypto.subtle.verify("HMAC", key, signatureBytes, signedBytes);
      },
      catch: (cause) =>
        new WebhookCryptoError({
          message: "Could not verify webhook signature",
          cause: causeName(cause),
        }),
    });

    if (!signatureIsValid)
      return yield* new InvalidWebhookSignatureError({
        message: "Webhook signature is invalid",
      });
  },
);

const decodeBody = Effect.fn("PhotonWebhook.decodeBody")((rawBody: string) =>
  Effect.gen(function* () {
    const parsed = yield* Schema.decodeUnknown(
      Schema.parseJson(Schema.Unknown),
    )(rawBody);

    const envelope = yield* Schema.decodeUnknown(PhotonWebhookEnvelope)(parsed);

    if (envelope.event !== "messages") return Option.none();

    const body = yield* Schema.decodeUnknown(PhotonWebhookBody)(parsed);

    return Option.some(body);
  }).pipe(
    Effect.mapError(
      () =>
        new InvalidWebhookBodyError({
          message: "Webhook body is malformed or unsupported",
        }),
    ),
  ),
);

const readBody = Effect.fn("PhotonWebhook.readBody")(
  (request: HttpServerRequest.HttpServerRequest) =>
    request.stream.pipe(
      Stream.mapError(
        () =>
          new InvalidWebhookBodyError({
            message: "Could not read webhook body",
          }),
      ),
      Stream.mapAccumEffect(0, (size, bytes) => {
        const nextSize = size + bytes.byteLength;

        if (nextSize > MAX_WEBHOOK_BODY_BYTES)
          return Effect.fail(
            new WebhookBodyTooLargeError({
              message: "Webhook body exceeds the allowed size",
            }),
          );

        return Effect.succeed([nextSize, bytes] as const);
      }),
      Stream.decodeText(),
      Stream.runCollect,
      Effect.map(Chunk.join("")),
    ),
);

const toVerifiedInboundMessage = (
  webhookId: string,
  body: PhotonWebhookBody,
): Option.Option<VerifiedInboundMessage> => {
  const { message, space } = body;

  if (
    message.platform !== "iMessage" ||
    message.direction !== "inbound" ||
    space.type !== "dm" ||
    message.sender === undefined ||
    message.content.type !== "text" ||
    message.content.text === undefined
  )
    return Option.none();

  const verified = {
    deliveryId: `${webhookId}:${message.id}`,
    messageId: message.id,
    platform: message.platform,
    senderId: message.sender.id,
    spaceId: space.id,
    text: message.content.text,
  };

  if (space.phone === undefined) return Option.some(verified);

  return Option.some({ ...verified, servingLine: space.phone });
};

/** Authenticate, decode, and narrow the current Photon request. */
export const verifyPhotonWebhook = Effect.fn("PhotonWebhook.verify")(
  function* () {
    const request = yield* HttpServerRequest.HttpServerRequest;

    const headers = yield* HttpServerRequest.schemaHeaders(
      PhotonWebhookHeaders,
    ).pipe(
      Effect.mapError(
        () =>
          new InvalidWebhookHeadersError({
            message: "Required Photon webhook headers are missing or malformed",
          }),
      ),
    );

    const timestamp = headers["x-spectrum-timestamp"];
    const signature = headers["x-spectrum-signature"];
    const webhookId = headers["x-spectrum-webhook-id"];

    yield* validateTimestamp(timestamp);

    const rawBody = yield* readBody(request);

    const webhookSecret = yield* webhookSecretConfig;

    yield* verifySignature({
      rawBody,
      secret: webhookSecret,
      signature,
      timestamp,
    });

    const event = headers["x-spectrum-event"];

    if (event !== undefined && event !== "messages") return Option.none();

    const body = yield* decodeBody(rawBody);

    return Option.flatMap(body, (value) =>
      toVerifiedInboundMessage(webhookId, value),
    );
  },
);

const respond = (status: number, body: string) =>
  Effect.succeed(HttpServerResponse.text(body, { status }));

/** Authenticate and translate one Photon webhook request. */
export const handlePhotonWebhook = Effect.fn("PhotonWebhook.handle")(
  function* () {
    const verified = yield* verifyPhotonWebhook();

    if (Option.isNone(verified))
      return HttpServerResponse.text("ignored", { status: 200 });

    // PR 2 routes this value to the Account Durable Object, where deliveryId
    // is claimed atomically with the account state transition.
    yield* Effect.logInfo("Accepted verified inbound message").pipe(
      Effect.annotateLogs({
        deliveryId: verified.value.deliveryId,
        messageId: verified.value.messageId,
        platform: verified.value.platform,
      }),
    );

    return HttpServerResponse.text("ok", { status: 200 });
  },
  Effect.catchTags({
    InvalidWebhookHeadersError: () =>
      respond(400, "missing or malformed headers"),
    InvalidWebhookTimestampError: () => respond(400, "invalid timestamp"),
    StaleWebhookTimestampError: () => respond(400, "stale timestamp"),
    InvalidWebhookSignatureError: () => respond(401, "invalid signature"),
    WebhookCryptoError: () => respond(500, "signature verification failed"),
    InvalidWebhookBodyError: () => respond(400, "invalid webhook body"),
    WebhookBodyTooLargeError: () => respond(413, "webhook body too large"),
  }),
);
