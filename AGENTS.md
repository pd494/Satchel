# satchel — agent instructions

## Working preferences

- Use available tools to inspect relevant files directly; do not ask the user to
  paste or show files that are accessible in the workspace.
- Continue non-destructive, non-sensitive operations within the requested scope
  without asking permission or stopping for confirmation.
- When a recommendation depends on verification, perform that verification as
  part of the current request. Do not stop at "we should confirm" or hand the
  investigation back to the user. Continue through available documentation,
  source inspection, and authorized checks. If verification is blocked, report
  the concrete missing access or resource and the evidence already established;
  never claim an unperformed live test passed.
- When the user is learning and asks to write code themselves, provide guidance
  without implementing it for them; still perform relevant read-only checks
  autonomously.

This is a [Spectrum](https://photon.codes/docs/spectrum-ts) app, pinned to `spectrum-ts@^12.8.0`. Reusable app assembly lives in `src/index.ts`. Configuration and provider setup live in `src/config.ts`; message streaming and response handling live in `src/connection.ts`. Keep typed errors beside the code that produces them and keep this small app's source layout flat. The current real-phone test entry point is `test/imessage.ts`.

## Working in this project

- Run the app with `bun start`.
- Add providers through `src/config.ts`.
- Outgoing message content uses the builders documented in the skill (text, attachment, voice, contact, richlink, poll, group, custom).

## Effect architecture

- Use `effect` for application orchestration, typed errors, logging, retries,
  scheduling, and dependency wiring across the project.
- Wrap Promise-based SDK calls with `Effect.tryPromise` at adapter boundaries.
- Define expected failures with `Schema.TaggedError`, including a `message` and
  specific context. Keep defects distinct.
- Use `Config` and `Config.redacted` rather than accessing `process.env`.
- Define real business services with `Effect.Service`, automatic accessors, and
  named `Effect.fn("Service.method")` operations.
- Compose service layers flatly with `Layer.mergeAll` or `Layer.provideMerge`.
- Use Effect `Schema` to decode unknown external input.
- Run Effects only at application entry points and platform handlers.
- Do not call `console` or create unmanaged timers in application modules.
- Cloudflare durable primitives remain responsible for persistence across
  invocations; Effect coordinates work within each invocation.

## Vendored repositories

This project vendors external source under `repos/` for agent reference.

- Before writing or reviewing Effect code, inspect `repos/effect` for idiomatic
  implementations, tests, module structure, and API usage.
- Read `agent-patterns/effect.md` for the project-specific starting patterns.
- Use the installed `effect-best-practices` skill for Effect services, schemas,
  errors, layers, configuration, and observability.
- Treat `repos/effect` as read-only reference material.
- Never import application code from `repos/effect`; continue importing the
  installed `effect` package.
- Do not edit vendored files unless the user explicitly asks.
- The subtree is pinned to the installed `effect@3.22.1` source tag. When the
  dependency changes, update both the package and subtree together.

## Environment

This project reads secrets from `.env` (gitignored). **Do not read, write, or echo `.env`** — it contains credentials.

If startup fails with an authentication error, tell the user to verify their `PROJECT_ID` / `PROJECT_SECRET` at the [Photon dashboard](https://app.photon.codes).

## Spectrum SDK reference

This project includes the `spectrum` skill from [`photon-hq/skills`](https://github.com/photon-hq/skills). Your agent should auto-discover it. If it doesn't, or if you switch agents, install for your agent with:

```sh
npx skills add photon-hq/skills --skill spectrum --agent <your-agent>
```

(Use `--agent '*'` to install for all supported agents.)

## Managing the Spectrum Cloud project (CLI)

If this app uses a platform provider, the `PROJECT_ID` / `PROJECT_SECRET` in `.env` belong to a **Spectrum Cloud** project. To manage that project from the terminal — authenticate, rotate the secret, list the line(s) you send from, manage platforms/users, or create more projects — use the `photon-cli` skill (the `photon` CLI) from [`photon-hq/skills`](https://github.com/photon-hq/skills):

```sh
npx skills add photon-hq/skills --skill photon-cli --agent <your-agent>
```

(Use `--agent '*'` to install for all supported agents.)

Common tasks once it's installed:

- `photon whoami` — confirm you're authenticated (run `photon login` if not).
- `photon projects regenerate-secret` — rotate the Spectrum API secret (then update `PROJECT_SECRET` in `.env`).
- `photon spectrum lines list` — see the line(s) your app sends from.
- `photon projects show` — inspect the active project (set `PHOTON_PROJECT_ID`, or pass `--project <id>`).

## See also

- [Spectrum docs](https://photon.codes/docs/spectrum-ts)
- [`spectrum-ts` on GitHub](https://github.com/photon-hq/spectrum-ts)
