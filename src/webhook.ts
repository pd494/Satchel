import { HttpServerRequest, HttpServerResponse } from "@effect/platform";
import { verifySpectrumSignature } from "@spectrum-ts/core/webhook";
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
import type { MessageAcceptanceError } from "./acceptMessage";
import {
  type AccountIdentity,
  DeliveryId,
  MessageId,
  SpaceId,
} from "./accountIdentity";

/** Required Photon webhook authentication headers were missing or malformed. */
class InvalidWebhookHeadersError extends Schema.TaggedError<InvalidWebhookHeadersError>()(
  "InvalidWebhookHeadersError",
  { message: Schema.String },
) {}

/** Photon webhook timestamp was not a valid Unix epoch second. */
class InvalidWebhookTimestampError extends Schema.TaggedError<InvalidWebhookTimestampError>()(
  "InvalidWebhookTimestampError",
  { message: Schema.String },
) {}

/** Webhook delivery was rejected because its timestamp is outside the tolerance window. */
class StaleWebhookTimestampError extends Schema.TaggedError<StaleWebhookTimestampError>()(
  "StaleWebhookTimestampError",
  { message: Schema.String },
) {}

/** Photon webhook signature did not authenticate the request. */
class InvalidWebhookSignatureError extends Schema.TaggedError<InvalidWebhookSignatureError>()(
  "InvalidWebhookSignatureError",
  { message: Schema.String },
) {}

/** Spectrum could not perform Photon webhook signature verification. */
class WebhookCryptoError extends Schema.TaggedError<WebhookCryptoError>()(
  "WebhookCryptoError",
  {
    operation: Schema.Literal("verifySpectrumSignature"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

/** The Worker could not load its required webhook secret binding. */
class WebhookConfigError extends Schema.TaggedError<WebhookConfigError>()(
  "WebhookConfigError",
  {
    operation: Schema.Literal("load WEBHOOK_SECRET"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

/** The request body stream failed before the complete delivery was read. */
class WebhookBodyReadError extends Schema.TaggedError<WebhookBodyReadError>()(
  "WebhookBodyReadError",
  {
    operation: Schema.Literal("request.stream"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

/** Authenticated Photon webhook body was not valid supported JSON. */
class InvalidWebhookBodyError extends Schema.TaggedError<InvalidWebhookBodyError>()(
  "InvalidWebhookBodyError",
  { message: Schema.String },
) {}

/** Photon webhook body exceeded the ingress limit. */
class WebhookBodyTooLargeError extends Schema.TaggedError<WebhookBodyTooLargeError>()(
  "WebhookBodyTooLargeError",
  { message: Schema.String },
) {}

const TIMESTAMP_TOLERANCE_SECONDS = 5 * 60;

const MAX_WEBHOOK_BODY_BYTES = 1024 * 1024;

const webhookSecretConfig = Config.redacted(
  Config.nonEmptyString("WEBHOOK_SECRET"),
);

const PhotonWebhookHeaders = Schema.Struct({
  "x-spectrum-event": Schema.String.pipe(Schema.minLength(1)),
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
  readonly deliveryId: DeliveryId;
  readonly messageId: MessageId;
  readonly platform: "imessage";
  readonly senderId: string;
  readonly spaceId: SpaceId;
  readonly servingLine?: string;
  readonly text: string;
}

const causeDescription = (cause: unknown): string =>
  cause instanceof Error ? `${cause.name}: ${cause.message}` : String(cause);

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
    readonly rawBody: Uint8Array;
    readonly secret: Redacted.Redacted<string>;
    readonly signature: string;
    readonly timestamp: string;
  }) {
    const now = yield* Clock.currentTimeMillis;

    const result = yield* Effect.tryPromise({
      try: () =>
        verifySpectrumSignature({
          headers: {
            "x-spectrum-signature": input.signature,
            "x-spectrum-timestamp": input.timestamp,
          },
          now,
          rawBody: input.rawBody,
          secret: Redacted.value(input.secret),
        }),
      catch: (cause) =>
        new WebhookCryptoError({
          operation: "verifySpectrumSignature",
          message: "Could not verify webhook signature",
          cause: causeDescription(cause),
        }),
    });

    if (result.ok) return;

    if (result.reason === "expired")
      return yield* new StaleWebhookTimestampError({
        message: "Webhook timestamp is outside the allowed window",
      });

    if (result.reason === "missing-headers")
      return yield* new InvalidWebhookTimestampError({
        message: "Webhook timestamp must be a Unix epoch second",
      });

    return yield* new InvalidWebhookSignatureError({
      message: "Webhook signature is invalid",
    });
  },
);

const decodeBody = Effect.fn("PhotonWebhook.decodeBody")(
  (rawBytes: Uint8Array) =>
    Effect.gen(function* () {
      const rawBody = yield* Effect.try({
        try: () => new TextDecoder("utf-8", { fatal: true }).decode(rawBytes),
        catch: () =>
          new InvalidWebhookBodyError({
            message: "Webhook body is not valid UTF-8",
          }),
      });

      const parsed = yield* Schema.decodeUnknown(
        Schema.parseJson(Schema.Unknown),
      )(rawBody);

      const envelope = yield* Schema.decodeUnknown(PhotonWebhookEnvelope)(
        parsed,
      );

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

const joinBytes = (chunks: Chunk.Chunk<Uint8Array>): Uint8Array => {
  const size = Chunk.reduce(
    chunks,
    0,
    (total, bytes) => total + bytes.byteLength,
  );

  const joined = new Uint8Array(size);
  let offset = 0;

  for (const bytes of chunks) {
    joined.set(bytes, offset);
    offset += bytes.byteLength;
  }

  return joined;
};

const readBody = Effect.fn("PhotonWebhook.readBody")(
  (request: HttpServerRequest.HttpServerRequest) =>
    request.stream.pipe(
      Stream.mapError(
        (cause) =>
          new WebhookBodyReadError({
            operation: "request.stream",
            message: "Could not read webhook body",
            cause: causeDescription(cause),
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
      Stream.runCollect,
      Effect.map(joinBytes),
    ),
);

const toVerifiedInboundMessage = (
  webhookId: string,
  body: PhotonWebhookBody,
): Option.Option<VerifiedInboundMessage> => {
  const { message, space } = body;

  if (
    message.platform !== "imessage" ||
    message.direction !== "inbound" ||
    space.type !== "dm" ||
    message.sender === undefined ||
    message.content.type !== "text" ||
    message.content.text === undefined
  )
    return Option.none();

  const verified: VerifiedInboundMessage = {
    deliveryId: DeliveryId.make(`${webhookId}:${message.id}`),
    messageId: MessageId.make(message.id),
    platform: message.platform,
    senderId: message.sender.id,
    spaceId: SpaceId.make(space.id),
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

    const webhookSecret = yield* webhookSecretConfig.pipe(
      Effect.mapError(
        (cause) =>
          new WebhookConfigError({
            operation: "load WEBHOOK_SECRET",
            message: "Required webhook secret is unavailable",
            cause: causeDescription(cause),
          }),
      ),
    );

    yield* verifySignature({
      rawBody,
      secret: webhookSecret,
      signature,
      timestamp,
    });

    const event = headers["x-spectrum-event"];

    if (event !== "messages") return Option.none();

    const body = yield* decodeBody(rawBody);

    return Option.flatMap(body, (value) =>
      toVerifiedInboundMessage(webhookId, value),
    );
  },
);

const respond = (status: number, body: string) =>
  Effect.succeed(HttpServerResponse.text(body, { status }));

type AcceptMessage = (
  message: VerifiedInboundMessage,
) => Effect.Effect<void, MessageAcceptanceError, AccountIdentity>;

const respondWithErrorLog = Effect.fn("PhotonWebhook.respondWithErrorLog")(
  function* (body: string, errorTag: string) {
    yield* Effect.logError("Webhook request failed").pipe(
      Effect.annotateLogs({ errorTag }),
    );

    return HttpServerResponse.text(body, { status: 500 });
  },
);

/** Authenticate and translate one Photon webhook request. */
export const handlePhotonWebhook = Effect.fn("PhotonWebhook.handle")(
  function* (acceptMessage: AcceptMessage) {
    const verified = yield* verifyPhotonWebhook();

    if (Option.isNone(verified))
      return HttpServerResponse.text("ignored", { status: 200 });

    yield* acceptMessage(verified.value);

    return HttpServerResponse.text("ok", { status: 200 });
  },
  Effect.catchTags({
    AccountIdConfigError: () =>
      respondWithErrorLog("message acceptance failed", "AccountIdConfigError"),
    AccountIdDerivationError: () =>
      respondWithErrorLog(
        "message acceptance failed",
        "AccountIdDerivationError",
      ),
    InboxStorageError: () =>
      respondWithErrorLog("message acceptance failed", "InboxStorageError"),
    InvalidWebhookHeadersError: () =>
      respond(400, "missing or malformed headers"),
    InvalidWebhookTimestampError: () => respond(400, "invalid timestamp"),
    StaleWebhookTimestampError: () => respond(400, "stale timestamp"),
    InvalidWebhookSignatureError: () => respond(401, "invalid signature"),
    WebhookCryptoError: () => respond(500, "signature verification failed"),
    WebhookConfigError: () => respond(500, "webhook is not configured"),
    WebhookBodyReadError: () => respond(400, "invalid webhook body"),
    InvalidWebhookBodyError: () => respond(400, "invalid webhook body"),
    WebhookBodyTooLargeError: () => respond(413, "webhook body too large"),
  }),
);
