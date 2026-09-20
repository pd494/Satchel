import { Effect, Schema } from "effect";
import { AccountIdentity } from "./accountIdentity";
import type { Account } from "./db";
import type { VerifiedInboundMessage } from "./webhook";

const safeCauseName = (cause: unknown): string =>
  cause instanceof Error ? cause.name : "Unknown rejection";

export class InboxStorageError extends Schema.TaggedError<InboxStorageError>()(
  "InboxStorageError",
  {
    operation: Schema.Literal("receiveMessage"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

/** Identify the private account and durably accept one verified delivery. */
export const acceptMessage = Effect.fn("Connection.acceptMessage")(function* (
  message: VerifiedInboundMessage,
  accounts: DurableObjectNamespace<Account>,
) {
  const accountId = yield* AccountIdentity.deriveAccountId(
    message.platform,
    message.senderId,
  );

  const account = accounts.get(accounts.idFromName(accountId));

  yield* Effect.tryPromise({
    try: () =>
      account.storeDeliveryOnce({
        deliveryId: message.deliveryId,
        messageId: message.messageId,
        text: message.text,
        spaceId: message.spaceId,
        platform: message.platform,
        servingLine: message.servingLine,
      }),
    catch: (cause) =>
      new InboxStorageError({
        operation: "receiveMessage",
        message: "Could not save the incoming message",
        cause: safeCauseName(cause),
      }),
  });

  yield* Effect.logInfo("Durably accepted inbound message").pipe(
    Effect.annotateLogs({
      deliveryId: message.deliveryId,
      messageId: message.messageId,
      platform: message.platform,
    }),
  );
});
