import { HttpServerResponse } from "@effect/platform";
import { Effect, Option } from "effect";
import type { InboxStorageError } from "./connection";
import { Hmac } from "./hmac";
import type {
  AccountIdConfigError,
  AccountIdDerivationError,
} from "./types/errors";
import type { VerifiedInboundMessage } from "./types/messages";

type AcceptMessage = (
  message: VerifiedInboundMessage,
) => Effect.Effect<
  void,
  AccountIdConfigError | AccountIdDerivationError | InboxStorageError,
  Hmac
>;

const respond = (status: number, body: string) =>
  Effect.succeed(HttpServerResponse.text(body, { status }));

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
    const verifiedMessage = yield* Hmac.verifyWebhook();

    if (Option.isNone(verifiedMessage))
      return HttpServerResponse.text("ignored", { status: 200 });

    yield* acceptMessage(verifiedMessage.value);

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
    WebhookCryptoError: () =>
      respondWithErrorLog(
        "signature verification failed",
        "WebhookCryptoError",
      ),
    WebhookConfigError: () =>
      respondWithErrorLog("webhook is not configured", "WebhookConfigError"),
    WebhookBodyReadError: () => respond(400, "invalid webhook body"),
    InvalidWebhookBodyError: () => respond(400, "invalid webhook body"),
    WebhookBodyTooLargeError: () => respond(413, "webhook body too large"),
  }),
);
