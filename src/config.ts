import { Config, Effect, Redacted, Schema } from "effect";
import { Spectrum } from "spectrum-ts";
import { imessage } from "spectrum-ts/providers/imessage";

const photonConfig = Config.all({
  projectId: Config.string("PROJECT_ID"),
  projectSecret: Config.redacted("PROJECT_SECRET"),
});

/** Failure to create the configured Spectrum application connection. */
export class PhotonConnectionError extends Schema.TaggedError<PhotonConnectionError>()(
  "PhotonConnectionError",
  {
    operation: Schema.Literal("Spectrum"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

const causeName = (cause: unknown): string =>
  cause instanceof Error ? cause.name : "Unknown rejection";

interface PhotonCredentials {
  readonly projectId: string;
  readonly projectSecret: string;
}

export type PhotonConnector = (
  credentials: PhotonCredentials,
) => ReturnType<typeof Spectrum>;

const connectSpectrum: PhotonConnector = ({ projectId, projectSecret }) =>
  Spectrum({
    projectId,
    projectSecret,
    providers: [imessage.config()],
  });

/** Connect to Photon using application configuration. */
export const connectPhoton = Effect.fn("Photon.connect")(function* (
  connect: PhotonConnector = connectSpectrum,
) {
  const config = yield* photonConfig;

  return yield* Effect.tryPromise({
    try: () =>
      connect({
        projectId: config.projectId,
        projectSecret: Redacted.value(config.projectSecret),
      }),
    catch: (cause) =>
      new PhotonConnectionError({
        operation: "Spectrum",
        message: "Could not connect to Photon",
        cause: causeName(cause),
      }),
  });
});
