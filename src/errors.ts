import { Schema } from "effect";

/** Failure while consuming the Spectrum application message stream. */
export class MessageStreamReadError extends Schema.TaggedError<MessageStreamReadError>()(
  "MessageStreamReadError",
  {
    operation: Schema.Literal("app.messages"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

/** Failure while sending a response through a Spectrum space. */
export class MessageSendError extends Schema.TaggedError<MessageSendError>()(
  "MessageSendError",
  {
    operation: Schema.Literal("space.send"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}
