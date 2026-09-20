import { Effect, Schema } from "effect";
import { AccountIdentity } from "./accountIdentity";
import type { Account } from "./db/account";
import type { VerifiedInboundMessage } from "./worker/photonWebhook";

const safeStorageCauseName = (cause: unknown): string =>
  cause instanceof Error ? cause.name : "Unknown rejection";

export class InboxStorageError extends Schema.TaggedError<InboxStorageError>()(
  "InboxStorageError",
  {
    operation: Schema.Literal("receiveMessage"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

/**
 * Accept a verified message into its sender's account.
 */
export const recvMessage = Effect.fn("Connection.recvMessage")(function* (
  _message: VerifiedInboundMessage,
  _accounts: DurableObjectNamespace<Account>,
) {
  const accountId = yield* AccountIdentity.deriveAccountId(
    _message.platform,
    _message.senderId,
  );

  const objectId = _accounts.idFromName(accountId);
  const account = _accounts.get(objectId);

  yield* Effect.tryPromise({
    try: () =>
      account.storeDeliveryOnce({
        deliveryId: _message.deliveryId,
        messageId: _message.messageId,
        text: _message.text,
        spaceId: _message.spaceId,
        platform: _message.platform,
        servingLine: _message.servingLine,
      }),
    catch: (cause) =>
      new InboxStorageError({
        operation: "receiveMessage",
        message: "Could not save the incoming message",
        cause: safeStorageCauseName(cause),
      }),
  });

  yield* Effect.logInfo("Durably accepted inbound message").pipe(
    Effect.annotateLogs({
      deliveryId: _message.deliveryId,
      messageId: _message.messageId,
      platform: _message.platform,
    }),
  );
});
