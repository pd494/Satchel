import { describe, expect, it } from "@effect/vitest";
import { ConfigProvider, Effect } from "effect";
import { AccountIdentity } from "../src/accounts/accountIdentity";

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
});
