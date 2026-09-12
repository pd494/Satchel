# satchel

A [Spectrum](https://photon.codes/docs/spectrum-ts) project. Wired with: imessage.
Application effects, typed failures, scheduling, and structured logs use
[Effect](https://effect.website/).

## Environment

Before running, open `.env` and fill in the values:

From your project Settings on the [Photon dashboard](https://app.photon.codes):

- `PROJECT_ID`
- `PROJECT_SECRET`

## Run

```sh
bun install
bun start
```

`bun start` currently runs the real-phone iMessage test in `test/imessage.ts`.
You can also run it explicitly with:

```sh
bun run test:imessage
```

## Automated checks

```sh
bun run lint
bun run check
bun run test
bun run test:coverage
bun run build
```

GitHub Actions runs independent quality, test, coverage, and build jobs. Quality
and test suites use matrices, so another check or test suite can be added by
adding its package script and one matrix entry. Shared Node, Bun, and dependency
setup lives in `.github/actions/setup-project/action.yml`.

The Skills matrix entry validates one uniquely named `SKILL.md` in every
`.agents/skills/*` directory and verifies the canonical anti-slop plugin path.
The quality job then loads and executes the generic and Effect anti-slop rules.

Coverage includes every `src/**/*.ts` file and publishes HTML and JSON reports
as a CI artifact. The current full-source baseline is 52.33% statements/lines,
75% branches, and 60% functions. The credential-free message-processing core
is 94.91% statements/lines, 85.71% branches, and 100% functions. CI enforces
the full-source baseline and a stronger 90% statements/lines threshold for the
core. Photon configuration and startup remain outside credential-free coverage
and are exercised by the separate real-phone smoke test rather than mocked SDK
modules.

Tests only discover `test/**/*.test.ts`; the real-phone runner is excluded.
They instantiate Spectrum with an in-process provider and exercise real message
wrapping, direction filtering, content dispatch, error propagation, and provider
shutdown without Photon credentials. They do not prove delivery through Photon
Cloud or Apple's iMessage network.

### Effect lint

`bun run lint:effect` runs the official
[@effect/language-service](https://github.com/Effect-TS/language-service)
diagnostics without patching TypeScript. `bun run check` runs it before the
TypeScript check, and CI executes that command as an independent quality-matrix
entry. The `--strict` flag makes warnings fail the job.

The default diagnostics remain enabled. `tsconfig.json` additionally enforces
typed error/requirements channels, safe Effect composition, service/layer
dependencies, Effect logging and configuration, and managed timers. It warns
on direct clock/random access and JSON operations so these use testable Effect
services and Schema decoding. SDK Promise boundaries and finite async test
iterators remain allowed. Style suggestions retain their upstream severities.

## Implementation plan

Follow the [messaging implementation plan](docs/IMPLEMENTATION_PLAN.md)
for the Cloudflare Worker, durable delivery, relay, and remaining real-phone
tests.

## Where to go next

- [Spectrum docs](https://photon.codes/docs/spectrum-ts)
- Edit `src/index.ts` to replace the echo loop with real agent logic.
- Add more providers from `spectrum-ts/providers/*`.
