import { env } from "cloudflare:test";
import { ConfigProvider, Effect } from "effect";
import { describe, expect, it, vi } from "vitest";
import {
  AccountIdentity,
  DeliveryId,
  MessageId,
  SpaceId,
} from "../src/accountIdentity";
import { InboxStorageError, recvMessage } from "../src/connection";

const testConfig = ConfigProvider.fromMap(
  new Map([["ACCOUNT_ID_SECRET", "test-account-id-secret"]]),
);

const message = {
  deliveryId: DeliveryId.make("test-delivery"),
  messageId: MessageId.make("test-message"),
  platform: "imessage",
  senderId: "test-sender",
  spaceId: SpaceId.make("test-space"),
  text: "test-message",
} as const;

describe("Message routing", () => {
  it("categorizes storage rejections without exposing details", async () => {
    const cases = [
      { rejection: new TypeError("private details"), cause: "TypeError" },
      { rejection: "private details", cause: "Unknown rejection" },
    ];

    for (const { rejection, cause } of cases) {
      const account = env.ACCOUNTS.getByName("storage-error-test");

      const storeDelivery = vi
        .spyOn(account, "storeDeliveryOnce")
        .mockRejectedValueOnce(rejection);

      const getAccount = vi.spyOn(env.ACCOUNTS, "get").mockReturnValue(account);

      const error = await Effect.runPromise(
        recvMessage(message, env.ACCOUNTS).pipe(
          Effect.provide(AccountIdentity.Default),
          Effect.withConfigProvider(testConfig),
          Effect.flip,
        ),
      );

      expect(error).toBeInstanceOf(InboxStorageError);
      expect(error.cause).toBe(cause);

      storeDelivery.mockRestore();
      getAccount.mockRestore();
    }
  });
});
