import { Config, Effect, Redacted, Schema } from "effect";

export const AccountId = Schema.String.pipe(Schema.brand("AccountId"));

export type AccountId = typeof AccountId.Type;

export const DeliveryId = Schema.String.pipe(Schema.brand("DeliveryId"));

export type DeliveryId = typeof DeliveryId.Type;

export const MessageId = Schema.String.pipe(Schema.brand("MessageId"));

export type MessageId = typeof MessageId.Type;

export const SpaceId = Schema.String.pipe(Schema.brand("SpaceId"));

export type SpaceId = typeof SpaceId.Type;

const accountIdSecret = Config.redacted(
  Config.nonEmptyString("ACCOUNT_ID_SECRET"),
);

const encodeIdentity = Schema.encodeSync(
  Schema.parseJson(Schema.Tuple(Schema.Literal("iMessage"), Schema.String)),
);

export class AccountIdDerivationError extends Schema.TaggedError<AccountIdDerivationError>()(
  "AccountIdDerivationError",
  {
    operation: Schema.Literal("HMAC-SHA256"),
    message: Schema.String,
    cause: Schema.String,
  },
) {}

export class AccountIdentity extends Effect.Service<AccountIdentity>()(
  "AccountIdentity",
  {
    accessors: true,
    effect: Effect.gen(function* () {
      const secret = yield* accountIdSecret;
      const encoder = new TextEncoder();

      const deriveAccountId = Effect.fn("AccountIdentity.deriveAccountId")(
        function* (platform: "iMessage", senderId: string) {
          const input = encoder.encode(encodeIdentity([platform, senderId]));

          const digest = yield* Effect.tryPromise({
            try: async () => {
              const key = await crypto.subtle.importKey(
                "raw",
                encoder.encode(Redacted.value(secret)),
                { name: "HMAC", hash: "SHA-256" },
                false,
                ["sign"],
              );

              return crypto.subtle.sign("HMAC", key, input);
            },
            catch: (cause) =>
              new AccountIdDerivationError({
                operation: "HMAC-SHA256",
                message: "Could not derive the account ID",
                cause:
                  cause instanceof Error ? cause.name : "Unknown rejection",
              }),
          });

          const accountId = Array.from(new Uint8Array(digest), (byte) =>
            byte.toString(16).padStart(2, "0"),
          ).join("");

          return AccountId.make(accountId);
        },
      );

      return { deriveAccountId };
    }),
  },
) {}
