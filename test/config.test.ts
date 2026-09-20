import { ConfigProvider, Effect } from "effect";
import { describe, expect, it } from "vitest";
import {
  connectPhoton,
  PhotonConnectionError,
  type PhotonConnector,
} from "../src/config";

const testConfig = ConfigProvider.fromMap(
  new Map([
    ["PROJECT_ID", "test-project"],
    ["PROJECT_SECRET", "test-secret"],
  ]),
);

const rejectWith =
  (cause: unknown): PhotonConnector =>
  () =>
    Promise.reject(cause);

const connect = (connector: PhotonConnector) =>
  Effect.runPromise(
    connectPhoton(connector).pipe(
      Effect.withConfigProvider(testConfig),
      Effect.flip,
    ),
  );

describe("Photon configuration", () => {
  it("maps Error rejections without exposing provider details", async () => {
    const error = await connect(
      rejectWith(new TypeError("private provider details")),
    );

    expect(error).toBeInstanceOf(PhotonConnectionError);

    if (!(error instanceof PhotonConnectionError)) return;

    expect(error.cause).toBe("TypeError");
  });

  it("maps non-Error rejections safely", async () => {
    const error = await connect(rejectWith("private provider details"));

    expect(error).toBeInstanceOf(PhotonConnectionError);

    if (!(error instanceof PhotonConnectionError)) return;

    expect(error.cause).toBe("Unknown rejection");
  });
});
