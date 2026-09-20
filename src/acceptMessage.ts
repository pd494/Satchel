import { Effect } from "effect";
import {
  type AccountIdConfigError,
  type AccountIdDerivationError,
  AccountIdentity,
} from "./accountIdentity";
import type { Account } from "./db";
import { InboxStorageError } from "./inboxErrors";
import type { VerifiedInboundMessage } from "./webhook";

export type MessageAcceptanceError =
  | AccountIdConfigError
  | AccountIdDerivationError
  | InboxStorageError;

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
      account.checkAndStoreMesage({
        deliveryId: message.deliveryId,
        messageId: message.messageId,
        text: message.text,
        spaceId: message.spaceId,
      }),
    catch: () =>
      new InboxStorageError({
        operation: "receiveMessage",
        message: "Could not save the incoming message",
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
