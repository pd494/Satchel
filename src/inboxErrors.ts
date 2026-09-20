import { Schema } from "effect";

export class InboxStorageError extends Schema.TaggedError<InboxStorageError>()(
  "InboxStorageError",
  {
    operation: Schema.Literal("receiveMessage"),
    message: Schema.String,
  },
) {}
