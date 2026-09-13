import { Effect } from "effect";
import { connectPhoton } from "./config";
import { type MessageHandler, recvMessage } from "./connection";

/** Run Satchel with the supplied message-processing behavior. */
export const runApp = Effect.fn("Satchel.run")(function* <E, R>(
  sendMessage: MessageHandler<E, R>,
) {
  const app = yield* connectPhoton();

  yield* Effect.logInfo("satchel.ready");
  yield* recvMessage(app, sendMessage);
});
