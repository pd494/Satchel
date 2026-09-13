import { imessage } from "@spectrum-ts/imessage";
import { Config, Effect, Redacted, Schema } from "effect";
import { Spectrum } from "spectrum-ts";

/** Photon project configuration loaded by Effect at application startup. */
export const PhotonConfig = Config.all({
  projectId: Config.string("PROJECT_ID"),
  projectSecret: Config.redacted("PROJECT_SECRET"),
});

/** Successfully parsed Photon project configuration. */
export type PhotonConfig = Config.Config.Success<typeof PhotonConfig>;

/** Failure to create the configured Spectrum application connection. */
export class PhotonConnectionError extends Schema.TaggedError<PhotonConnectionError>()(
  "PhotonConnectionError",
  {
    operation: Schema.String,
    message: Schema.String,
    cause: Schema.optional(Schema.String),
  },
) {}

function makePhotonApp(config: PhotonConfig) {
  return Spectrum({
    projectId: config.projectId,
    projectSecret: Redacted.value(config.projectSecret),
    providers: [imessage.config()],
  });
}

/** Connected Spectrum application type produced by the Photon adapter. */
export type PhotonApp = Awaited<ReturnType<typeof makePhotonApp>>;

const causeName = (cause: unknown): string =>
  cause instanceof Error ? cause.name : "Unknown rejection";

/** Create a Spectrum application while mapping SDK rejection into a typed error. */
export const createPhotonApp = Effect.fn("PhotonAdapter.createApp")(function* (
  config: PhotonConfig,
) {
  return yield* Effect.tryPromise({
    try: () => makePhotonApp(config),
    catch: (cause) =>
      new PhotonConnectionError({
        operation: "Spectrum",
        message: "Could not connect to Photon",
        cause: causeName(cause),
      }),
  });
});
