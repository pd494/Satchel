# Effect patterns for Satchel

The installed package and vendored reference are both Effect 3.22.1.
Application code imports from `effect`; it never imports from `repos/effect`.

Before introducing an Effect API, search its implementation, documentation,
and tests under `repos/effect/packages/effect/`.

## Promise boundaries

Wrap Promise APIs at adapters with `Effect.tryPromise` and map rejection values
to a serializable `Schema.TaggedError`.

```ts
class ProviderSendError extends Schema.TaggedError<ProviderSendError>()(
  "ProviderSendError",
  {
    operation: Schema.String,
    message: Schema.String,
    cause: Schema.optional(Schema.String)
  }
) {}

const send = Effect.tryPromise({
  try: () => provider.send(),
  catch: (cause) => new ProviderSendError({
    operation: "send",
    message: "Provider send failed",
    cause: cause instanceof Error ? cause.name : "Unknown rejection"
  })
})
```

References:

- `repos/effect/packages/effect/src/Effect.ts`
- `repos/effect/packages/effect/test/Effect/tryPromise.test.ts`
- `repos/effect/packages/effect/test/Effect/error.test.ts`

## Async iterables

Use `Stream.fromAsyncIterable` instead of writing a manual iterator loop. Map
stream failures into the typed error channel, then consume with a Stream sink.

```ts
const messages = Stream.fromAsyncIterable(source, (cause) =>
  new ProviderError({ operation: "read stream", cause })
)

const program = messages.pipe(Stream.runForEach(handleMessage))
```

References:

- `repos/effect/packages/effect/src/Stream.ts`
- `repos/effect/packages/effect/test/Stream/constructors.test.ts`

## Errors

- Use `Schema.TaggedError` for expected failures.
- Give every error a `message` and a specific, recoverable meaning.
- Include a stable operation name and the original cause at external boundaries.
- Recover with `Effect.catchTag` when behavior depends on one error type.
- Prefer `catchTag` or `catchTags` over broad `catchAll` and `mapError`.
- Do not catch defects as ordinary operational failures.

## Services and layers

- Define business services with `Effect.Service` and `accessors: true`.
- Wrap service operations with `Effect.fn("Service.method")`.
- Declare service dependencies in the service definition.
- Use `Layer.mergeAll` for peers and `Layer.provideMerge` for incremental
  composition.
- Use `Context.Tag` only for runtime-provided infrastructure such as Cloudflare
  bindings.

## Configuration

- Read process configuration with `Config` rather than `process.env`.
- Use `Config.redacted` for secrets and unwrap only at the external SDK call.
- Provide Worker bindings at the Worker entry point.

## Logging

- Use `Effect.logInfo`, `Effect.logWarning`, and `Effect.logError`.
- Attach safe identifiers with `Effect.annotateLogs`.
- Never log message bodies, filenames, file bytes, secrets, or phone numbers.

Reference: `repos/effect/packages/effect/src/Logger.ts`.

## Concurrency and time

- Use `Effect.sleep` rather than unmanaged timers.
- Prefer scoped fibers for owned background work.
- Use `Effect.forkDaemon` only for work intentionally owned by the application
  runtime rather than the current request.
- Cloudflare Durable Objects or Workflows provide persistence across runtime
  termination; an Effect fiber alone is not durable.

## Runtime boundaries

Compose Effects throughout application code and call `Effect.runPromise` only
at process or platform entry points. Use Layers once a real dependency needs a
test implementation or has an owned lifecycle; do not create empty services.

## Tests

When Effect test tooling is added, inspect existing tests under
`repos/effect/packages/effect/test/` first. Prefer `it.effect` with
`@effect/vitest` for Effect programs.

Run both TypeScript and Effect-specific diagnostics with:

```sh
bun run check
```
