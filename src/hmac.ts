import { HttpServerRequest } from "@effect/platform";
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
import {
  AccountIdConfigError,
  AccountIdDerivationError,
  InvalidWebhookBodyError,
  InvalidWebhookHeadersError,
  InvalidWebhookSignatureError,
  InvalidWebhookTimestampError,
  StaleWebhookTimestampError,
  WebhookBodyReadError,
  WebhookBodyTooLargeError,
  WebhookConfigError,
  WebhookCryptoError,
} from "./types/errors";
import type { SignatureInput, VerifiedInboundMessage } from "./types/messages";
import {
  AccountId,
  DeliveryId,
  MessageId,
  PhotonWebhookBody,
  PhotonWebhookEnvelope,
  PhotonWebhookHeaders,
  SpaceId,
} from "./types/messages";

/** Verify Photon deliveries and derive account IDs, each using its own secret. */
export class Hmac extends Effect.Service<Hmac>()("Hmac", {
  accessors: true,
  sync: () => {
    const operations = {
      /** Authenticate, decode, and narrow the current Photon request. */
      verifyWebhook: Effect.fn("Hmac.verifyWebhook")(function* () {
        const request = yield* HttpServerRequest.HttpServerRequest;

        const headers = yield* HttpServerRequest.schemaHeaders(
          PhotonWebhookHeaders,
        ).pipe(
          Effect.mapError(
            () =>
              new InvalidWebhookHeadersError({
                message:
                  "Required Photon webhook headers are missing or malformed",
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

        if (event !== "messages") {
          yield* Effect.logInfo("Ignored Photon webhook").pipe(
            Effect.annotateLogs({
              event,
              reason: "unsupported-header-event",
            }),
          );

          return Option.none();
        }

        const body = yield* decodeBody(rawBody);

        if (Option.isNone(body)) return Option.none();

        return yield* toVerifiedInboundMessage(webhookId, body.value);
      }),

      deriveAccountId: Effect.fn("Hmac.deriveAccountId")(function* (
        platform: "imessage",
        senderId: string,
      ) {
        const secret = yield* accountIdSecret.pipe(
          Effect.mapError(
            (cause) =>
              new AccountIdConfigError({
                operation: "load ACCOUNT_ID_SECRET",
                message: "Required account identity secret is unavailable",
                cause: causeDescription(cause),
              }),
          ),
        );

        const encoder = new TextEncoder();
        const input = encoder.encode(encodeIdentity([platform, senderId]));

        const digest = yield* Effect.tryPromise({
          try: async () => {
            const key = await crypto.subtle.importKey(
              "raw",
              encoder.encode(Redacted.value(secret)),
              { name: "HMAC", hash: "SHA-256" },
              false,
              ["sign"],
            );

            return crypto.subtle.sign("HMAC", key, input);
          },
          catch: (cause) =>
            new AccountIdDerivationError({
              operation: "HMAC-SHA256",
              message: "Could not derive the account ID",
              cause: cause instanceof Error ? cause.name : "Unknown rejection",
            }),
        });

        const accountId = Array.from(new Uint8Array(digest), (byte) =>
          byte.toString(16).padStart(2, "0"),
        ).join("");

        return AccountId.make(accountId);
      }),
    };

    const accountIdSecret = Config.redacted(
      Config.nonEmptyString("ACCOUNT_ID_SECRET"),
    );

    const encodeIdentity = Schema.encodeSync(
      Schema.parseJson(Schema.Tuple(Schema.Literal("imessage"), Schema.String)),
    );

    const TIMESTAMP_TOLERANCE_SECONDS = 5 * 60;

    const MAX_WEBHOOK_BODY_BYTES = 1024 * 1024;

    const webhookSecretConfig = Config.redacted(
      Config.nonEmptyString("WEBHOOK_SECRET"),
    );

    const causeDescription = (cause: unknown): string =>
      cause instanceof Error
        ? `${cause.name}: ${cause.message}`
        : String(cause);

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
      function* (input: SignatureInput) {
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
            try: () =>
              new TextDecoder("utf-8", { fatal: true }).decode(rawBytes),
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

          if (envelope.event !== "messages") {
            yield* Effect.logInfo("Ignored Photon webhook").pipe(
              Effect.annotateLogs({
                event: envelope.event,
                reason: "unsupported-body-event",
              }),
            );

            return Option.none();
          }

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

    const toVerifiedInboundMessage = Effect.fn(
      "PhotonWebhook.toVerifiedInboundMessage",
    )(function* (webhookId: string, body: PhotonWebhookBody) {
      const { message, space } = body;

      if (
        message.platform !== "imessage" ||
        message.direction !== "inbound" ||
        space.type !== "dm" ||
        message.sender === undefined ||
        message.content.type !== "text" ||
        message.content.text === undefined
      ) {
        yield* Effect.logInfo("Ignored unsupported Photon message").pipe(
          Effect.annotateLogs({
            contentType: message.content.type,
            direction: message.direction,
            hasSender: message.sender !== undefined,
            hasText: message.content.text !== undefined,
            platform: message.platform,
            spaceType: space.type ?? "missing",
          }),
        );

        return Option.none();
      }

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
    });

    return operations;
  },
}) {}
