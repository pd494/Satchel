import { Schema } from "effect";

/** Failure while consuming the Spectrum application message stream. */
export class MessageStreamReadError extends Schema.TaggedError<MessageStreamReadError>()(
  "MessageStreamReadError",
  {
    operation: Schema.Literal("app.messages"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

/** Failure while sending a response through a Spectrum space. */
export class MessageSendError extends Schema.TaggedError<MessageSendError>()(
  "MessageSendError",
  {
    operation: Schema.Literal("space.send"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

/** Required Photon webhook authentication headers were missing or malformed. */
export class InvalidWebhookHeadersError extends Schema.TaggedError<InvalidWebhookHeadersError>()(
  "InvalidWebhookHeadersError",
  {
    message: Schema.String,
  },
) {}

/** Photon webhook timestamp was not a valid Unix epoch second. */
export class InvalidWebhookTimestampError extends Schema.TaggedError<InvalidWebhookTimestampError>()(
  "InvalidWebhookTimestampError",
  {
    message: Schema.String,
  },
) {}

/** Webhook delivery was rejected because its timestamp is outside the tolerance window. */
export class StaleWebhookTimestampError extends Schema.TaggedError<StaleWebhookTimestampError>()(
  "StaleWebhookTimestampError",
  {
    message: Schema.String,
  },
) {}

/** Photon webhook signature was malformed or did not authenticate the request. */
export class InvalidWebhookSignatureError extends Schema.TaggedError<InvalidWebhookSignatureError>()(
  "InvalidWebhookSignatureError",
  {
    message: Schema.String,
  },
) {}

/** Web Crypto could not perform Photon webhook signature verification. */
export class WebhookCryptoError extends Schema.TaggedError<WebhookCryptoError>()(
  "WebhookCryptoError",
  {
    message: Schema.String,
    cause: Schema.String,
  },
) {}

/** Authenticated Photon webhook body was not valid supported JSON. */
export class InvalidWebhookBodyError extends Schema.TaggedError<InvalidWebhookBodyError>()(
  "InvalidWebhookBodyError",
  {
    message: Schema.String,
  },
) {}

/** Photon webhook body exceeded the ingress limit. */
export class WebhookBodyTooLargeError extends Schema.TaggedError<WebhookBodyTooLargeError>()(
  "WebhookBodyTooLargeError",
  {
    message: Schema.String,
  },
) {}
