import { Effect } from "effect";
import { connectPhoton } from "./config";
import {
  type MessageHandler,
  type MessageSource,
  recvMessage,
} from "./connection";

/** A Spectrum message source whose provider clients can be released. */
export interface MessageApplication extends MessageSource {
  readonly stop: () => Promise<void>;
}

/** Acquire, run, and always release a Spectrum application connection. */
export const runAppWith = Effect.fn("Satchel.runWith")(
  <ConnectError, ConnectRequirements, HandlerError, HandlerRequirements>(
    connect: Effect.Effect<
      MessageApplication,
      ConnectError,
      ConnectRequirements
    >,
    sendMessage: MessageHandler<HandlerError, HandlerRequirements>,
  ) =>
    Effect.acquireUseRelease(
      connect,
      (app) =>
        Effect.logInfo("satchel.ready").pipe(
          Effect.zipRight(recvMessage(app, sendMessage)),
        ),
      (app) => Effect.promise(() => app.stop()),
    ),
);

/** Run Satchel with the supplied message-processing behavior. */
export const runApp = Effect.fn("Satchel.run")(function* <E, R>(
  sendMessage: MessageHandler<E, R>,
) {
  yield* runAppWith(connectPhoton(), sendMessage);
});
