import { ConfigProvider, Effect } from "effect";
import { describe, expect, it, vi } from "vitest";
import {
  AccountIdConfigError,
  AccountIdDerivationError,
  AccountIdentity,
} from "../src/accountIdentity";

const testConfig = ConfigProvider.fromMap(
  new Map([["ACCOUNT_ID_SECRET", "test-account-id-secret"]]),
);

describe("Account identity", () => {
  it("derives the same account ID for the same sender", () =>
    Effect.runPromise(
      Effect.gen(function* () {
        const first = yield* AccountIdentity.deriveAccountId(
          "imessage",
          "test-sender",
        );

        const second = yield* AccountIdentity.deriveAccountId(
          "imessage",
          "test-sender",
        );

        expect(first).toBe(second);
      }).pipe(
        Effect.provide(AccountIdentity.Default),
        Effect.withConfigProvider(testConfig),
      ),
    ));

  it("derives a different account ID for a different sender", () =>
    Effect.runPromise(
      Effect.gen(function* () {
        const first = yield* AccountIdentity.deriveAccountId(
          "imessage",
          "first-sender",
        );

        const second = yield* AccountIdentity.deriveAccountId(
          "imessage",
          "second-sender",
        );

        expect(first).not.toBe(second);
      }).pipe(
        Effect.provide(AccountIdentity.Default),
        Effect.withConfigProvider(testConfig),
      ),
    ));

  it("preserves a typed missing-secret failure", async () => {
    const error = await Effect.runPromise(
      AccountIdentity.deriveAccountId("imessage", "test-sender").pipe(
        Effect.provide(AccountIdentity.Default),
        Effect.withConfigProvider(ConfigProvider.fromMap(new Map())),
        Effect.flip,
      ),
    );

    expect(error).toBeInstanceOf(AccountIdConfigError);
    expect(error.operation).toBe("load ACCOUNT_ID_SECRET");
  });

  it("maps crypto rejections without exposing details", async () => {
    const cases = [
      { rejection: new TypeError("private details"), cause: "TypeError" },
      { rejection: "private details", cause: "Unknown rejection" },
    ];

    for (const { rejection, cause } of cases) {
      const importKey = vi
        .spyOn(crypto.subtle, "importKey")
        .mockRejectedValueOnce(rejection);

      const error = await Effect.runPromise(
        AccountIdentity.deriveAccountId("imessage", "test-sender").pipe(
          Effect.provide(AccountIdentity.Default),
          Effect.withConfigProvider(testConfig),
          Effect.flip,
        ),
      );

      importKey.mockRestore();

      expect(error).toBeInstanceOf(AccountIdDerivationError);
      expect(error.cause).toBe(cause);
    }
  });
});
