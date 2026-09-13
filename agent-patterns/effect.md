# Effect patterns for Satchel

The installed Effect package is pinned to 3.22.1. Application code imports from
`effect`; it never imports internal files from `node_modules/effect/src`.

Before introducing an Effect API, search its implementation, documentation,
and types under `node_modules/effect/src`. Consult the official Effect
repository at the matching tag only when upstream tests are needed.

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

- `node_modules/effect/src/Effect.ts`

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

- `node_modules/effect/src/Stream.ts`

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

Reference: `node_modules/effect/src/Logger.ts`.

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

Prefer `it.effect` with `@effect/vitest` for Effect programs. When behavior is
unclear, consult tests in the official Effect repository at the tag matching
the installed package.

Run both TypeScript and Effect-specific diagnostics with:

```sh
bun run check
```
