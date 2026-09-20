import type { DurableObjectNamespace } from "@cloudflare/workers-types";
import { describe, expect, it } from "@effect/vitest";
import { afterAll, beforeAll, vi } from "vitest";
import { createTestHarness, type TestHarness } from "wrangler";
import type { Account } from "../src/db";

describe("Account inbox", () => {
  let server: TestHarness;

  beforeAll(async () => {
    vi.stubEnv("CLOUDFLARE_LOAD_DEV_VARS_FROM_DOT_ENV", "false");
    server = createTestHarness({
      root: process.cwd(),
      workers: [{ configPath: "./wrangler.jsonc" }],
    });
    await server.listen();
  });

  afterAll(async () => {
    try {
      await server?.close();
    } finally {
      vi.unstubAllEnvs();
    }
  });

  /**
   * The first accepted message should start onboarding; later messages should not.
   * Retrying a delivery must preserve its original content and create no extra work.
   */
  it("marks first contact once and ignores duplicate deliveries", async () => {
    const worker = server.getWorker<{
      ACCOUNTS: DurableObjectNamespace<Account>;
    }>();

    const { ACCOUNTS } = await worker.getEnv();

    const account = ACCOUNTS.getByName("inbox-test");

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

    const storage = await server
      .getWorker()
      .getDurableObjectStorage("Account", {
        name: "inbox-test",
      });

    expect(
      await storage.exec(
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
    const { ACCOUNTS } = await server
      .getWorker<{
        ACCOUNTS: DurableObjectNamespace<Account>;
      }>()
      .getEnv();

    const account = ACCOUNTS.getByName("concurrent");

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

    const storage = await server
      .getWorker()
      .getDurableObjectStorage("Account", { name: "concurrent" });

    expect(
      await storage.exec(
        "SELECT COUNT(*) AS total, SUM(is_first_message) AS first_messages FROM inbox",
      ),
    ).toEqual([{ total: 2, first_messages: 1 }]);
  });

  /**
   * A late retry must not reopen finished work or change its text or reply route.
   * Seed a completed row, retry it, and verify the entire saved row is unchanged.
   */
  it("does not overwrite or requeue a completed delivery", async () => {
    const { ACCOUNTS } = await server
      .getWorker<{
        ACCOUNTS: DurableObjectNamespace<Account>;
      }>()
      .getEnv();

    const account = ACCOUNTS.getByName("completed");

    const message = {
      deliveryId: "done",
      messageId: "original",
      text: "hello",
      spaceId: "space",
    };

    await account.checkAndStoreMesage(message);

    const storage = await server
      .getWorker()
      .getDurableObjectStorage("Account", { name: "completed" });

    // Seed completion through Wrangler's public storage API until processing exists.
    await storage.exec(
      "UPDATE inbox SET status = 'completed' WHERE delivery_id = ?",
      message.deliveryId,
    );

    const before = await storage.exec("SELECT * FROM inbox");

    await account.checkAndStoreMesage({
      ...message,
      text: "retry",
      spaceId: "different-space",
    });
    expect(await storage.exec("SELECT * FROM inbox")).toEqual(before);
  });
});
