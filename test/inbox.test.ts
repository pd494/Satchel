import { env, evictDurableObject, runInDurableObject } from "cloudflare:test";
import { ConfigProvider, Effect } from "effect";
import { describe, expect, it } from "vitest";
import {
  AccountIdentity,
  DeliveryId,
  MessageId,
  SpaceId,
} from "../src/accounts/accountIdentity";
import { safeCauseName } from "../src/inboxErrors";

const identityConfig = ConfigProvider.fromMap(
  new Map([["ACCOUNT_ID_SECRET", "test-account-id-secret"]]),
);

/** Read account storage inside the Durable Object's own runtime context. */
const queryInbox = <Row extends Record<string, SqlStorageValue>>(
  account: DurableObjectStub,
  query: string,
): Promise<Row[]> =>
  runInDurableObject(account, (_instance, state) =>
    state.storage.sql.exec<Row>(query).toArray(),
  );

describe("Account inbox", () => {
  it("records safe storage rejection categories", () => {
    expect(safeCauseName(new TypeError("private details"))).toBe("TypeError");
    expect(safeCauseName("private details")).toBe("Unknown rejection");
  });

  /**
   * The first accepted message should start onboarding; later messages should not.
   * Retrying a delivery must preserve its original content and create no extra work.
   */
  it("marks first contact once and ignores duplicate deliveries", async () => {
    const account = env.ACCOUNTS.getByName("inbox-test");

    const first = {
      deliveryId: DeliveryId.make("delivery-1"),
      messageId: MessageId.make("message-1"),
      text: "hello",
      spaceId: SpaceId.make("space-1"),
      platform: "imessage" as const,
      servingLine: "line-1",
    };

    await account.storeDeliveryOnce(first);
    await account.storeDeliveryOnce({
      ...first,
      text: "retry must not overwrite",
      servingLine: "retry must not overwrite",
    });
    await account.storeDeliveryOnce({
      ...first,
      deliveryId: DeliveryId.make("delivery-2"),
      messageId: MessageId.make("message-2"),
      text: "second message",
      servingLine: undefined,
    });

    expect(
      await queryInbox<{
        delivery_id: string;
        text: string;
        platform: string;
        serving_line: string | null;
        is_first_message: number;
        status: string;
      }>(
        account,
        "SELECT delivery_id, text, platform, serving_line, is_first_message, status FROM inbox ORDER BY sequence",
      ),
    ).toEqual([
      {
        delivery_id: "delivery-1",
        text: "hello",
        platform: "imessage",
        serving_line: "line-1",
        is_first_message: 1,
        status: "pending",
      },
      {
        delivery_id: "delivery-2",
        text: "second message",
        platform: "imessage",
        serving_line: null,
        is_first_message: 0,
        status: "pending",
      },
    ]);
  });

  /**
   * Overlapping arrivals must not trigger onboarding twice or save a retry twice.
   * Send two distinct deliveries and a duplicate concurrently to exercise both rules.
   */
  it("accepts concurrent deliveries once and marks exactly one first message", async () => {
    const account = env.ACCOUNTS.getByName("concurrent");

    const message = {
      deliveryId: DeliveryId.make("a"),
      messageId: MessageId.make("a"),
      text: "hello",
      spaceId: SpaceId.make("space"),
      platform: "imessage" as const,
    };

    await Promise.all([
      account.storeDeliveryOnce(message),
      account.storeDeliveryOnce(message),
      account.storeDeliveryOnce({
        ...message,
        deliveryId: DeliveryId.make("b"),
        messageId: MessageId.make("b"),
      }),
    ]);

    expect(
      await queryInbox<{ total: number; first_messages: number }>(
        account,
        "SELECT COUNT(*) AS total, SUM(is_first_message) AS first_messages FROM inbox",
      ),
    ).toEqual([{ total: 2, first_messages: 1 }]);
  });

  /**
   * A late retry must not reopen finished work or change its text or reply route.
   * Seed a completed row, retry it, and verify the entire saved row is unchanged.
   */
  it("does not overwrite or requeue a completed delivery", async () => {
    const account = env.ACCOUNTS.getByName("completed");

    const message = {
      deliveryId: DeliveryId.make("done"),
      messageId: MessageId.make("original"),
      text: "hello",
      spaceId: SpaceId.make("space"),
      platform: "imessage" as const,
    };

    await account.storeDeliveryOnce(message);

    const before = await runInDurableObject(account, (_instance, state) => {
      state.storage.sql.exec(
        "UPDATE inbox SET status = 'completed' WHERE delivery_id = ?",
        message.deliveryId,
      );

      return state.storage.sql.exec("SELECT * FROM inbox").toArray();
    });

    await account.storeDeliveryOnce({
      ...message,
      text: "retry",
      spaceId: SpaceId.make("different-space"),
    });

    expect(await queryInbox(account, "SELECT * FROM inbox")).toEqual(before);
  });

  it("preserves accepted work across object eviction", async () => {
    const account = env.ACCOUNTS.getByName("restart");

    await account.storeDeliveryOnce({
      deliveryId: DeliveryId.make("before-restart"),
      messageId: MessageId.make("before-restart"),
      text: "before",
      spaceId: SpaceId.make("space"),
      platform: "imessage",
    });

    await evictDurableObject(account);

    await account.storeDeliveryOnce({
      deliveryId: DeliveryId.make("after-restart"),
      messageId: MessageId.make("after-restart"),
      text: "after",
      spaceId: SpaceId.make("space"),
      platform: "imessage",
    });

    expect(
      await queryInbox<{ total: number; first_messages: number }>(
        account,
        "SELECT COUNT(*) AS total, SUM(is_first_message) AS first_messages FROM inbox",
      ),
    ).toEqual([{ total: 2, first_messages: 1 }]);
  });

  it("keeps different senders in isolated account databases", async () => {
    const [firstId, secondId] = await Effect.runPromise(
      Effect.all([
        AccountIdentity.deriveAccountId("imessage", "first-sender"),
        AccountIdentity.deriveAccountId("imessage", "second-sender"),
      ]).pipe(
        Effect.provide(AccountIdentity.Default),
        Effect.withConfigProvider(identityConfig),
      ),
    );

    const first = env.ACCOUNTS.getByName(firstId);
    const second = env.ACCOUNTS.getByName(secondId);

    await first.storeDeliveryOnce({
      deliveryId: DeliveryId.make("first-delivery"),
      messageId: MessageId.make("first-message"),
      text: "first",
      spaceId: SpaceId.make("first-space"),
      platform: "imessage",
    });
    await second.storeDeliveryOnce({
      deliveryId: DeliveryId.make("second-delivery"),
      messageId: MessageId.make("second-message"),
      text: "second",
      spaceId: SpaceId.make("second-space"),
      platform: "imessage",
    });

    expect(
      await queryInbox<{ text: string }>(first, "SELECT text FROM inbox"),
    ).toEqual([{ text: "first" }]);
    expect(
      await queryInbox<{ text: string }>(second, "SELECT text FROM inbox"),
    ).toEqual([{ text: "second" }]);
  });
});
