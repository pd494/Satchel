import { describe, expect, it } from "@effect/vitest";
import { Effect } from "effect";
import {
  type Content,
  definePlatform,
  Spectrum,
  UnsupportedError,
} from "spectrum-ts";
import z from "zod";
import { recvMessage, sendMessage } from "../src/connection";
import { MessageSendError, MessageStreamReadError } from "../src/errors";
import { runAppWith } from "../src/index";

type Input = {
  readonly id: string;
  readonly content: Content;
  readonly direction: "inbound" | "outbound";
};

type Failure = "none" | "read" | "send";

const input = (
  id: string,
  direction: Input["direction"],
  content: Content,
): Input => ({ id, direction, content });

const makeHarness = async (
  inbound: ReadonlyArray<Input>,
  failure: Failure = "none",
) => {
  const sent: Array<string> = [];
  const state = { sent, closed: false, sendAttempts: 0, stopped: false };

  const provider = definePlatform("test_harness", {
    config: z.object({}),
    lifecycle: {
      createClient: () => Promise.resolve(state),
      destroyClient: ({ client }) => {
        client.stopped = true;

        return Promise.resolve();
      },
    },
    user: {
      resolve: ({ input }) => Promise.resolve({ id: input.userID }),
    },
    space: {
      create: () => Promise.resolve({ id: "test-space" }),
    },
    messages: async function* ({ client }) {
      try {
        for (const message of inbound) {
          yield { ...message, space: { id: "test-space" } };
        }

        if (failure === "read") {
          throw new TypeError("private provider details");
        }
      } finally {
        client.closed = true;
      }
    },
    send: ({ client, content, space }) => {
      client.sendAttempts += 1;

      if (failure === "send" && client.sendAttempts === 1) {
        return Promise.reject(new TypeError("private provider details"));
      }

      if (content.type !== "text") {
        return Promise.reject(
          UnsupportedError.content(content.type, "test_harness"),
        );
      }

      client.sent.push(content.text);

      return Promise.resolve({
        id: `sent-${client.sent.length}`,
        content,
        direction: "outbound" as const,
        space,
      });
    },
  });

  const app = await Spectrum({
    providers: [provider.config()],
    options: { logLevel: "silent" },
  });

  return { app, state };
};

const stop = (app: Awaited<ReturnType<typeof makeHarness>>["app"]) =>
  Effect.promise(() => app.stop());

describe("Spectrum message flow", () => {
  it.effect(
    "echoes inbound text through Spectrum's real dispatch pipeline",
    () =>
      Effect.gen(function* () {
        const { app, state } = yield* Effect.promise(() =>
          makeHarness([
            input("own-message", "outbound", {
              type: "text",
              text: "ignore",
            }),
            input("first", "inbound", { type: "text", text: "first" }),
            input("typing", "inbound", { type: "typing", state: "start" }),
            input("second", "inbound", { type: "text", text: "second" }),
          ]),
        );

        yield* recvMessage(app, sendMessage).pipe(Effect.ensuring(stop(app)));

        expect(state.sent).toEqual(["echo: first", "echo: second"]);
        expect(state.closed).toBe(true);
        expect(state.stopped).toBe(true);
      }),
  );

  it.effect("preserves a typed send failure", () =>
    Effect.gen(function* () {
      const { app, state } = yield* Effect.promise(() =>
        makeHarness(
          [input("first", "inbound", { type: "text", text: "hello" })],
          "send",
        ),
      );

      const result = yield* Effect.promise(() =>
        app.messages[Symbol.asyncIterator]().next(),
      );

      expect(result.done).toBe(false);

      if (result.done) {
        return;
      }

      const error = yield* sendMessage(result.value).pipe(
        Effect.ensuring(stop(app)),
        Effect.flip,
      );

      expect(error).toBeInstanceOf(MessageSendError);
      expect(error.operation).toBe("space.send");
      expect(error.cause).toBe("TypeError");
      expect(state.closed).toBe(true);
      expect(state.stopped).toBe(true);
    }),
  );

  it.effect("continues receiving after a send failure", () =>
    Effect.gen(function* () {
      const { app, state } = yield* Effect.promise(() =>
        makeHarness(
          [
            input("first", "inbound", { type: "text", text: "first" }),
            input("second", "inbound", { type: "text", text: "second" }),
          ],
          "send",
        ),
      );

      yield* recvMessage(app, sendMessage).pipe(Effect.ensuring(stop(app)));

      expect(state.sendAttempts).toBe(2);
      expect(state.sent).toEqual(["echo: second"]);
      expect(state.closed).toBe(true);
      expect(state.stopped).toBe(true);
    }),
  );

  it.effect("maps a stream failure and releases Spectrum", () =>
    Effect.gen(function* () {
      const { app, state } = yield* Effect.promise(() =>
        makeHarness([], "read"),
      );

      const error = yield* runAppWith(Effect.succeed(app), sendMessage).pipe(
        Effect.flip,
      );

      expect(error).toBeInstanceOf(MessageStreamReadError);
      expect(error.operation).toBe("app.messages");
      expect(error.cause).toBe("TypeError");
      expect(state.closed).toBe(true);
      expect(state.stopped).toBe(true);
    }),
  );
});
