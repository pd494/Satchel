import { Config, Effect, Redacted } from "effect";
import { Spectrum } from "spectrum-ts";
import { imessage } from "spectrum-ts/providers/imessage";
import { PhotonConnectionError } from "./types/errors";

const photonConfig = Config.all({
  projectId: Config.string("PROJECT_ID"),
  projectSecret: Config.redacted("PROJECT_SECRET"),
});

const causeName = (cause: unknown): string =>
  cause instanceof Error ? cause.name : "Unknown rejection";

/** Connect to Photon using application configuration. */
export const connectPhoton = Effect.fn("Photon.connect")(function* () {
  const config = yield* photonConfig;

  return yield* Effect.tryPromise({
    try: () =>
      Spectrum({
        projectId: config.projectId,
        projectSecret: Redacted.value(config.projectSecret),
        providers: [imessage.config()],
      }),
    catch: (cause) =>
      new PhotonConnectionError({
        operation: "Spectrum",
        message: "Could not connect to Photon",
        cause: causeName(cause),
      }),
  });
});
