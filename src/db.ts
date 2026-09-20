import { DurableObject } from "cloudflare:workers";
import { sql } from "drizzle-orm";
import {
  type DrizzleSqliteDODatabase,
  drizzle,
} from "drizzle-orm/durable-sqlite";
import {
  check,
  index,
  integer,
  sqliteTable,
  text,
} from "drizzle-orm/sqlite-core";
import { DateTime, Effect } from "effect";
import { InboxStorageError } from "./inboxErrors";
import type { WorkerBindings } from "./worker";

export interface InboxMessage {
  readonly deliveryId: string;
  readonly messageId: string;
  readonly text: string;
  readonly spaceId: string;
}

const inbox = sqliteTable(
  "inbox",
  {
    sequence: integer("sequence").primaryKey({ autoIncrement: true }),
    deliveryId: text("delivery_id").notNull().unique(),
    messageId: text("message_id").notNull(),
    text: text("text").notNull(),
    spaceId: text("space_id").notNull(),
    isFirstMessage: integer("is_first_message", { mode: "boolean" }).notNull(),
    status: text("status", { enum: ["pending", "completed"] })
      .notNull()
      .default("pending"),
    createdAt: integer("created_at", { mode: "timestamp_ms" }).notNull(),
  },
  (table) => [
    index("inbox_status_sequence_idx").on(table.status, table.sequence),
    check("inbox_first_message_check", sql`${table.isFirstMessage} IN (0, 1)`),
    check(
      "inbox_status_check",
      sql`${table.status} IN ('pending', 'completed')`,
    ),
  ],
);

const accountSchema = { inbox };

export class Account extends DurableObject<WorkerBindings> {
  readonly db: DrizzleSqliteDODatabase<typeof accountSchema>;

  constructor(ctx: DurableObjectState, env: WorkerBindings) {
    super(ctx, env);

    ctx.storage.sql.exec(`
      CREATE TABLE IF NOT EXISTS inbox (
        sequence INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
        delivery_id TEXT NOT NULL UNIQUE,
        message_id TEXT NOT NULL,
        text TEXT NOT NULL,
        space_id TEXT NOT NULL,
        is_first_message INTEGER NOT NULL CHECK (is_first_message IN (0, 1)),
        status TEXT DEFAULT 'pending' NOT NULL CHECK (status IN ('pending', 'completed')),
        created_at INTEGER NOT NULL
      );

      CREATE INDEX IF NOT EXISTS inbox_status_sequence_idx
        ON inbox(status, sequence);
    `);

    this.db = drizzle(ctx.storage, {
      logger: false,
      schema: accountSchema,
    });
  }

  /** Store a delivery once and mark whether it began this account's inbox. */
  async checkAndStoreMesage(message: InboxMessage): Promise<void> {
    const db = this.db;

    return Effect.runPromise(
      Effect.gen(function* () {
        const now = yield* DateTime.now;

        yield* Effect.try({
          try: () =>
            db.transaction((tx) => {
              const existing = tx
                .select({ sequence: accountSchema.inbox.sequence })
                .from(accountSchema.inbox)
                .limit(1)
                .get();

              tx.insert(accountSchema.inbox)
                .values({
                  deliveryId: message.deliveryId,
                  messageId: message.messageId,
                  text: message.text,
                  spaceId: message.spaceId,
                  isFirstMessage: existing === undefined,
                  createdAt: DateTime.toDateUtc(now),
                })
                .onConflictDoNothing({ target: accountSchema.inbox.deliveryId })
                .run();
            }),
          catch: () =>
            new InboxStorageError({
              operation: "receiveMessage",
              message: "Could not save the incoming message",
            }),
        });
      }),
    );
  }
}
