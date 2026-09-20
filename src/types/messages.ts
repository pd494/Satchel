import { type Redacted, Schema } from "effect";

export const AccountId = Schema.String.pipe(Schema.brand("AccountId"));

export type AccountId = typeof AccountId.Type;

export const DeliveryId = Schema.String.pipe(Schema.brand("DeliveryId"));

export type DeliveryId = typeof DeliveryId.Type;

export const MessageId = Schema.String.pipe(Schema.brand("MessageId"));

export type MessageId = typeof MessageId.Type;

export const SpaceId = Schema.String.pipe(Schema.brand("SpaceId"));

export type SpaceId = typeof SpaceId.Type;

export interface SignatureInput {
  readonly rawBody: Uint8Array;
  readonly secret: Redacted.Redacted<string>;
  readonly signature: string;
  readonly timestamp: string;
}

export const PhotonWebhookHeaders = Schema.Struct({
  "x-spectrum-event": Schema.String.pipe(Schema.minLength(1)),
  "x-spectrum-signature": Schema.String.pipe(Schema.minLength(1)),
  "x-spectrum-timestamp": Schema.String.pipe(Schema.minLength(1)),
  "x-spectrum-webhook-id": Schema.String.pipe(Schema.minLength(1)),
});

export const PhotonWebhookEnvelope = Schema.Struct({
  event: Schema.String,
});

export const PhotonWebhookBody = Schema.Struct({
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

export type PhotonWebhookBody = typeof PhotonWebhookBody.Type;

export interface VerifiedInboundMessage {
  readonly deliveryId: DeliveryId;
  readonly messageId: MessageId;
  readonly platform: "imessage";
  readonly senderId: string;
  readonly spaceId: SpaceId;
  readonly servingLine?: string;
  readonly text: string;
}

/** Verified fields persisted for later processing; excludes the raw sender. */
export interface InboxMessage {
  readonly deliveryId: DeliveryId;
  readonly messageId: MessageId;
  readonly text: string;
  readonly spaceId: SpaceId;
  readonly platform: "imessage";
  readonly servingLine?: string;
}
