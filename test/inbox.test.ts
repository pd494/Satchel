import { env, runInDurableObject } from "cloudflare:test";
import { describe, expect, it } from "vitest";

/** Read account storage inside the Durable Object's own runtime context. */
const queryInbox = <Row extends Record<string, SqlStorageValue>>(
  account: DurableObjectStub,
  query: string,
): Promise<Row[]> =>
  runInDurableObject(account, (_instance, state) =>
    state.storage.sql.exec<Row>(query).toArray(),
  );

describe("Account inbox", () => {
  /**
   * The first accepted message should start onboarding; later messages should not.
   * Retrying a delivery must preserve its original content and create no extra work.
   */
  it("marks first contact once and ignores duplicate deliveries", async () => {
    const account = env.ACCOUNTS.getByName("inbox-test");

    const first = {
      deliveryId: "delivery-1",
      messageId: "message-1",
      text: "hello",
      spaceId: "space-1",
    };

    await account.checkAndStoreMesage(first);
    await account.checkAndStoreMesage({
      ...first,
      text: "retry must not overwrite",
    });
    await account.checkAndStoreMesage({
      ...first,
      deliveryId: "delivery-2",
      messageId: "message-2",
      text: "second message",
    });

    expect(
      await queryInbox<{
        delivery_id: string;
        text: string;
        is_first_message: number;
        status: string;
      }>(
        account,
        "SELECT delivery_id, text, is_first_message, status FROM inbox ORDER BY sequence",
      ),
    ).toEqual([
      {
        delivery_id: "delivery-1",
        text: "hello",
        is_first_message: 1,
        status: "pending",
      },
      {
        delivery_id: "delivery-2",
        text: "second message",
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
      deliveryId: "a",
      messageId: "a",
      text: "hello",
      spaceId: "space",
    };

    await Promise.all([
      account.checkAndStoreMesage(message),
      account.checkAndStoreMesage(message),
      account.checkAndStoreMesage({
        ...message,
        deliveryId: "b",
        messageId: "b",
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
      deliveryId: "done",
      messageId: "original",
      text: "hello",
      spaceId: "space",
    };

    await account.checkAndStoreMesage(message);

    const before = await runInDurableObject(account, (_instance, state) => {
      state.storage.sql.exec(
        "UPDATE inbox SET status = 'completed' WHERE delivery_id = ?",
        message.deliveryId,
      );

      return state.storage.sql.exec("SELECT * FROM inbox").toArray();
    });

    await account.checkAndStoreMesage({
      ...message,
      text: "retry",
      spaceId: "different-space",
    });

    expect(await queryInbox(account, "SELECT * FROM inbox")).toEqual(before);
  });
});
