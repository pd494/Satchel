import { describe, expect, it } from "@effect/vitest";
import { ConfigProvider, Effect } from "effect";
import { vi } from "vitest";
import {
  AccountIdConfigError,
  AccountIdDerivationError,
  AccountIdentity,
} from "../src/accountIdentity";

const testConfig = ConfigProvider.fromMap(
  new Map([["ACCOUNT_ID_SECRET", "test-account-id-secret"]]),
);

describe("Account identity", () => {
  it.effect("derives the same account ID for the same sender", () =>
    Effect.gen(function* () {
      const first = yield* AccountIdentity.deriveAccountId(
        "iMessage",
        "test-sender",
      );

      const second = yield* AccountIdentity.deriveAccountId(
        "iMessage",
        "test-sender",
      );

      expect(first).toBe(second);
    }).pipe(
      Effect.provide(AccountIdentity.Default),
      Effect.withConfigProvider(testConfig),
    ),
  );

  it.effect("derives a different account ID for a different sender", () =>
    Effect.gen(function* () {
      const first = yield* AccountIdentity.deriveAccountId(
        "iMessage",
        "first-sender",
      );

      const second = yield* AccountIdentity.deriveAccountId(
        "iMessage",
        "second-sender",
      );

      expect(first).not.toBe(second);
    }).pipe(
      Effect.provide(AccountIdentity.Default),
      Effect.withConfigProvider(testConfig),
    ),
  );

  it.effect("preserves a typed missing-secret failure", () =>
    Effect.gen(function* () {
      const error = yield* AccountIdentity.deriveAccountId(
        "iMessage",
        "test-sender",
      ).pipe(Effect.flip);

      expect(error).toBeInstanceOf(AccountIdConfigError);
      expect(error.operation).toBe("load ACCOUNT_ID_SECRET");
    }).pipe(
      Effect.provide(AccountIdentity.Default),
      Effect.withConfigProvider(ConfigProvider.fromMap(new Map())),
    ),
  );

  it.effect("maps crypto rejections without exposing details", () =>
    Effect.gen(function* () {
      const cases = [
        { rejection: new TypeError("private details"), cause: "TypeError" },
        { rejection: "private details", cause: "Unknown rejection" },
      ];

      for (const { rejection, cause } of cases) {
        const importKey = vi
          .spyOn(crypto.subtle, "importKey")
          .mockRejectedValueOnce(rejection);

        const error = yield* AccountIdentity.deriveAccountId(
          "iMessage",
          "test-sender",
        ).pipe(Effect.flip);

        importKey.mockRestore();

        expect(error).toBeInstanceOf(AccountIdDerivationError);
        expect(error.cause).toBe(cause);
      }
    }).pipe(
      Effect.provide(AccountIdentity.Default),
      Effect.withConfigProvider(testConfig),
    ),
  );
});
