import { Schema } from "effect";

export const safeCauseName = (cause: unknown): string =>
  cause instanceof Error ? cause.name : "Unknown rejection";

export class InboxStorageError extends Schema.TaggedError<InboxStorageError>()(
  "InboxStorageError",
  {
    operation: Schema.Literal("receiveMessage"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}
