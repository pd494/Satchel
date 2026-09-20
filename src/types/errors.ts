import { Schema } from "effect";

export class AccountIdConfigError extends Schema.TaggedError<AccountIdConfigError>()(
  "AccountIdConfigError",
  {
    operation: Schema.Literal("load ACCOUNT_ID_SECRET"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

export class AccountIdDerivationError extends Schema.TaggedError<AccountIdDerivationError>()(
  "AccountIdDerivationError",
  {
    operation: Schema.Literal("HMAC-SHA256"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

export class InvalidWebhookBodyError extends Schema.TaggedError<InvalidWebhookBodyError>()(
  "InvalidWebhookBodyError",
  { message: Schema.String },
) {}

export class InvalidWebhookHeadersError extends Schema.TaggedError<InvalidWebhookHeadersError>()(
  "InvalidWebhookHeadersError",
  { message: Schema.String },
) {}

export class InvalidWebhookSignatureError extends Schema.TaggedError<InvalidWebhookSignatureError>()(
  "InvalidWebhookSignatureError",
  { message: Schema.String },
) {}

export class InvalidWebhookTimestampError extends Schema.TaggedError<InvalidWebhookTimestampError>()(
  "InvalidWebhookTimestampError",
  { message: Schema.String },
) {}

export class PhotonConnectionError extends Schema.TaggedError<PhotonConnectionError>()(
  "PhotonConnectionError",
  {
    operation: Schema.Literal("Spectrum"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

export class StaleWebhookTimestampError extends Schema.TaggedError<StaleWebhookTimestampError>()(
  "StaleWebhookTimestampError",
  { message: Schema.String },
) {}

export class WebhookBodyReadError extends Schema.TaggedError<WebhookBodyReadError>()(
  "WebhookBodyReadError",
  {
    operation: Schema.Literal("request.stream"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

export class WebhookBodyTooLargeError extends Schema.TaggedError<WebhookBodyTooLargeError>()(
  "WebhookBodyTooLargeError",
  { message: Schema.String },
) {}

export class WebhookConfigError extends Schema.TaggedError<WebhookConfigError>()(
  "WebhookConfigError",
  {
    operation: Schema.Literal("load WEBHOOK_SECRET"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

export class WebhookCryptoError extends Schema.TaggedError<WebhookCryptoError>()(
  "WebhookCryptoError",
  {
    operation: Schema.Literal("verifySpectrumSignature"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}
