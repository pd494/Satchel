import { Effect } from "effect";
import { createPhotonApp, PhotonConfig } from "./config";
import { type MessageHandler, recvMessage } from "./connection";

/** Run Satchel with the supplied message-processing behavior. */
export const runApp = Effect.fn("Satchel.run")(function* <E, R>(
  sendMessage: MessageHandler<E, R>,
) {
  const config = yield* PhotonConfig;
  const app = yield* createPhotonApp(config);

  yield* Effect.logInfo("satchel.ready");
  yield* recvMessage(app, sendMessage);
});
