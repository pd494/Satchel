import { Effect, Stream } from "effect";
import type { Message, Space } from "spectrum-ts";
import { MessageSendError, MessageStreamReadError } from "./errors";

/** One conversation-and-message pair emitted by Spectrum. */
export type MessageEntry = readonly [Space, Message];

/** The narrow Spectrum capability required by the receive loop. */
export interface MessageSource {
  readonly messages: AsyncIterable<MessageEntry>;
}

/** Effectful behavior applied to each inbound Spectrum message. */
export type MessageHandler<E, R> = (
  entry: MessageEntry,
) => Effect.Effect<void, E, R>;

const causeName = (cause: unknown): string =>
  cause instanceof Error ? cause.name : "Unknown rejection";

/** Send the current test response for one inbound Spectrum message. */
export const sendMessage = Effect.fn("Connection.sendMessage")(function* ([
  space,
  message,
]: MessageEntry) {
  if (message.content.type !== "text") {
    return;
  }

  const text = message.content.text;

  yield* Effect.logInfo("Received an inbound message");
  yield* Effect.tryPromise({
    try: () => space.send(`echo: ${text}`),
    catch: (cause) =>
      new MessageSendError({
        operation: "space.send",
        message: "Could not send the test reply",
        cause: causeName(cause),
      }),
  });
});

/** Consume inbound Spectrum messages in order with the supplied handler. */
export const recvMessage = Effect.fn("Connection.recvMessage")(function* <E, R>(
  app: MessageSource,
  sendMessage: MessageHandler<E, R>,
) {
  yield* Stream.fromAsyncIterable(
    app.messages,
    (cause) =>
      new MessageStreamReadError({
        operation: "app.messages",
        message: "The Spectrum message stream failed",
        cause: causeName(cause),
      }),
  ).pipe(
    Stream.filter(([, message]) => message.direction === "inbound"),
    Stream.runForEach((entry) =>
      sendMessage(entry).pipe(
        Effect.tapError((cause) =>
          Effect.logError("Could not process inbound message").pipe(
            Effect.annotateLogs({
              cause: causeName(cause),
              messageId: entry[1].id,
              platform: entry[1].platform,
            }),
          ),
        ),
        Effect.ignore,
      ),
    ),
  );
});
