import { DurableObject } from "cloudflare:workers";
import { DateTime, Effect, Schema } from "effect";
import type { WorkerBindings } from "./types/bindings";
import type { InboxMessage } from "./types/messages";

const safeCauseName = (cause: unknown): string =>
  cause instanceof Error ? cause.name : "Unknown rejection";

export class InboxDatabaseError extends Schema.TaggedError<InboxDatabaseError>()(
  "InboxDatabaseError",
  {
    operation: Schema.Literal("storeDeliveryOnce"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

export class Account extends DurableObject<WorkerBindings> {
  constructor(ctx: DurableObjectState, env: WorkerBindings) {
    super(ctx, env);

    ctx.storage.sql.exec(`
      CREATE TABLE IF NOT EXISTS inbox (
        sequence INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
        delivery_id TEXT NOT NULL UNIQUE,
        message_id TEXT NOT NULL,
        text TEXT NOT NULL,
        space_id TEXT NOT NULL,
        platform TEXT NOT NULL CHECK (platform = 'imessage'),
        serving_line TEXT,
        is_first_message INTEGER NOT NULL CHECK (is_first_message IN (0, 1)),
        status TEXT DEFAULT 'pending' NOT NULL CHECK (status IN ('pending', 'completed')),
        created_at INTEGER NOT NULL
      );

      CREATE INDEX IF NOT EXISTS inbox_status_sequence_idx
        ON inbox(status, sequence);
    `);
  }

  /** Store a delivery once and mark whether it began this account's inbox. */
  async storeDeliveryOnce(message: InboxMessage): Promise<void> {
    const storage = this.ctx.storage;

    return Effect.runPromise(
      Effect.gen(function* () {
        const now = yield* DateTime.now;

        yield* Effect.try({
          try: () =>
            storage.transactionSync(() => {
              const existing = storage.sql
                .exec<{ sequence: number }>(
                  "SELECT sequence FROM inbox LIMIT 1",
                )
                .toArray()[0];

              storage.sql.exec(
                `INSERT INTO inbox (
                  delivery_id,
                  message_id,
                  text,
                  space_id,
                  platform,
                  serving_line,
                  is_first_message,
                  created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(delivery_id) DO NOTHING`,
                message.deliveryId,
                message.messageId,
                message.text,
                message.spaceId,
                message.platform,
                message.servingLine ?? null,
                existing === undefined ? 1 : 0,
                DateTime.toEpochMillis(now),
              );
            }),
          catch: (cause) =>
            new InboxDatabaseError({
              operation: "storeDeliveryOnce",
              message: "Could not save the incoming message",
              cause: safeCauseName(cause),
            }),
        });
      }),
    );
  }
}
