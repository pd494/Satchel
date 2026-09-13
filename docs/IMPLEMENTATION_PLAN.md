# Satchel messaging implementation plan

This guide turns the M1 Photon proof into a durable first production architecture.
Work from top to bottom. Do not start the next phase until the checkpoint for the
current phase passes.

Source task: [Prove Photon delivery and choose the messaging adapter](https://app.notion.com/p/3d668f2b916d8196a639fe74dd67bb29)

## Current status

- [x] `spectrum-ts@12.8.0` connects to the iMessage Cloud provider.
- [x] `effect@3.22.1` drives configuration, errors, logging, scheduling, and
      message I/O.
- [x] Text replies are implemented.
- [x] A five-second delayed reply reached a real phone.
- [x] Inbound file streams were downloaded with nonzero byte counts.
- [ ] The production Worker and webhook do not exist yet.
- [ ] Delays do not survive a restart yet.
- [ ] Files are not persisted or sent back yet.
- [ ] End-to-end security, duplicate, reconnect, and retry tests remain.

## Architecture decision

Photon's public webhook is Worker-compatible for inbound events. The current
public Photon documentation says outbound sends and attachment downloads still
require a long-lived `spectrum-ts` process. The installed iMessage provider also
uses gRPC, which is a Bun/Node transport rather than a Cloudflare Workers
transport.

Use this split:

```text
Inbound
Phone -> iMessage Cloud -> Spectrum webhook -> Cloudflare Worker
                                           -> Conversation Durable Object

Outbound
Conversation Durable Object -> authenticated HTTP job -> Bun relay
                                                  Bun relay -> spectrum-ts
                                                            -> iMessage Cloud
                                                            -> Phone

Files
Photon attachment metadata -> Worker -> Bun relay retrieves bytes -> R2
R2 -> Bun relay -> spectrum-ts attachment send -> Phone
```

The Worker owns durable application state. The relay is intentionally small: it
only translates authenticated jobs into `spectrum-ts` calls and retrieves
attachments.

## Rules that keep the design clean

1. Model application work as `Effect<Success, Error, Requirements>`.
2. Wrap Promise-based SDK calls with `Effect.tryPromise` at adapter boundaries.
3. Use specific `Schema.TaggedError` failures with messages instead of throwing
   untyped application errors.
4. Use Effect logging and annotations instead of calling `console` directly.
5. Run Effects only at entry points: Bun, Worker, Durable Object, and relay
   handlers.
6. Use `Config.redacted` for secrets and unwrap only at the SDK boundary.
7. Define business services with `Effect.Service` and named `Effect.fn`
   operations.
8. Keep Photon-specific code in `adapters/photon.ts` and the relay.
9. Keep message orchestration in `services/msg-io.ts`.
10. Keep HTTP parsing and responses in `routes/`.
11. Treat Photon IDs as opaque strings.
12. Never log message text, filenames, file contents, secrets, or phone numbers.
13. Persist state before acknowledging work that must survive interruption.
14. Assume every webhook, alarm, and network request can happen more than once.
15. Do not claim exactly-once delivery. Make each operation idempotent instead.
16. Add a file or abstraction only when the phase actively uses it.
17. Keep `.env` local and gitignored. Cloudflare secrets belong in secret
    bindings, not `wrangler.jsonc`.

Effect is the application-code architecture, not the durable storage mechanism.
Cloudflare persists state and performs wake-ups. Effect provides typed errors,
dependency wiring, retries, timeouts, logging, and resource safety inside each
invocation.

## Phase 1: Worker ingress

**Goal:** receive and authenticate Photon webhooks without changing the working
Bun smoke test.

### Implement

- [ ] Add Wrangler and Cloudflare Worker types as development dependencies.
- [ ] Add `src/worker.ts` as the Worker entry point.
- [ ] Add `src/routes/photon-webhook.ts`.
- [ ] Extend `src/types/environment.ts` with Worker bindings.
- [ ] Decode unknown webhook JSON with Effect `Schema` before it reaches
      `msg-io.ts`.
- [ ] Express webhook verification as an Effect with tagged security errors.
- [ ] Add `wrangler.jsonc` with no secret values.
- [ ] Keep `src/index.ts` and `bun start` as the local Spectrum smoke test.
- [ ] Add separate `worker:dev`, `worker:check`, and `worker:deploy` scripts.

### Webhook behavior

The route must:

1. Read the request body exactly once as raw text.
2. Require `X-Spectrum-Event`, `X-Spectrum-Webhook-Id`,
   `X-Spectrum-Timestamp`, and `X-Spectrum-Signature`.
3. Reject timestamps more than five minutes away from the current time.
4. Compute HMAC-SHA256 over `v0:{timestamp}:{rawBody}`.
5. Compare the expected and received signatures without early-exit comparison.
6. Parse JSON only after verification succeeds.
7. Accept only the `messages` event initially.

Use the Web Crypto API in the Worker instead of importing `node:crypto`.

### Checkpoint

- [ ] `GET /health` returns `200`.
- [ ] Missing signature headers return `400`.
- [ ] An invalid signature returns `401`.
- [ ] A stale timestamp is rejected.
- [ ] A valid captured fixture reaches `msg-io.ts`.
- [ ] `bun start` still type-checks and retains its current behavior.

Reference: [Photon webhook quickstart](https://photon.codes/docs/webhooks/quickstart)

## Phase 2: Durable inbox and scheduling

**Goal:** repeated webhooks produce one logical job, and delayed work survives a
Worker restart or deployment. No SQL migration file is required.

### Implement

- [ ] Add `src/infrastructure/conversation-state.ts` as a Durable Object.
- [ ] Bind one Durable Object instance per opaque `space.id`.
- [ ] Store processed inbound IDs under `inbox:{message.id}`.
- [ ] Store outbound jobs under `job:{jobId}`.
- [ ] Use `reply:{message.id}` as the deterministic reply job ID.
- [ ] Store job status, attempt count, scheduled time, and last safe error code.
- [ ] Wrap Durable Object operations in an Effect service at the infrastructure
      boundary; do not leak Cloudflare storage calls into `msg-io.ts`.
- [ ] Use Durable Object storage methods such as `get`, `put`, and `delete`;
      keep SQL out of application code.
- [ ] Set an alarm for the earliest scheduled job.
- [ ] Make the alarm handler safe to run more than once.
- [ ] Remove old inbox keys after the documented deduplication window.

Suggested job states:

```text
pending -> sending -> sent
                   -> failed
                   -> unknown
```

`unknown` means the network result was ambiguous. Do not automatically send the
same message again unless the transport can prove idempotency across that retry.

### Checkpoint

- [ ] Posting the same `message.id` twice creates one reply job.
- [ ] A scheduled job still runs after restarting local Worker development.
- [ ] A duplicate alarm invocation does not create a second job.
- [ ] A successful job cannot transition back to `pending`.

References:

- [Durable Object storage](https://developers.cloudflare.com/durable-objects/best-practices/access-durable-objects-storage/)
- [Durable Object alarms](https://developers.cloudflare.com/durable-objects/api/alarms/)

## Phase 3: Authenticated Spectrum relay

**Goal:** allow the Worker to request an outbound send or attachment download
through the supported long-lived Spectrum runtime.

### Implement

- [ ] Add a separate `relay/` package with its own entry point.
- [ ] Move or reuse the existing Spectrum setup from `adapters/photon.ts`.
- [ ] Expose only these internal operations initially:
  - [ ] send text to a sender/conversation
  - [ ] send an attachment
  - [ ] retrieve an inbound attachment by ID and line
- [ ] Authenticate Worker-to-relay requests with a separate relay secret.
- [ ] Sign the method, path, timestamp, job ID, and raw body.
- [ ] Reject stale relay requests and repeated job IDs.
- [ ] Validate request bodies before calling Spectrum.
- [ ] Build relay handlers as Effect programs and provide their dependencies
      through Effect layers.
- [ ] Rebuild iMessage DMs from the opaque sender address as Photon documents.
- [ ] Return a provider message ID when Spectrum confirms a send.
- [ ] Return a distinct ambiguous/unknown result for uncertain timeouts.
- [ ] Add graceful shutdown and structured error handling.

Do not expose a general-purpose proxy and do not accept arbitrary Spectrum
method names from the request body.

### Checkpoint

- [ ] An unsigned request cannot call the relay.
- [ ] A stale signed request cannot call the relay.
- [ ] Repeating a completed relay job ID does not send twice.
- [ ] The Worker can request one real text reply through the relay.
- [ ] Restarting the relay reconnects to Spectrum and accepts pending work.

Reference: [Photon webhook reply guidance](https://photon.codes/docs/webhooks/quickstart#reply-from-your-handler-optional)

## Phase 4: File round trip

**Goal:** persist inbound files and send files back through iMessage.

### Implement

- [ ] Add an R2 binding.
- [ ] Add `src/adapters/r2.ts` only now, when it becomes used.
- [ ] Store files using generated object keys, never user filenames as keys.
- [ ] Store safe metadata separately: message ID, MIME type, byte count, hash,
      and R2 key.
- [ ] Have the relay stream inbound attachment bytes to an authenticated Worker
      endpoint that writes to R2.
- [ ] Enforce maximum size and allowed MIME-type rules before storage.
- [ ] Use Effect `Stream` when it materially improves file backpressure and
      cleanup; do not buffer large attachments by default.
- [ ] Stream outbound files from R2 to the relay rather than buffering large
      files in the Worker.
- [ ] Send one small image and one PDF back through Spectrum.

### Checkpoint

- [ ] An inbound image is stored with the expected byte count and checksum.
- [ ] An inbound PDF is stored and opens correctly.
- [ ] The returned image opens on a real phone.
- [ ] The returned PDF opens on a real phone.
- [ ] Failed downloads and oversized files have safe, visible job states.

Reference: [Photon webhook event and attachment format](https://photon.codes/docs/webhooks/events)

## Phase 5: Reliability tests

**Goal:** prove the boundaries that the SDK does not prove for Satchel.

### Automated tests

- [ ] Valid webhook fixture is accepted.
- [ ] Invalid signature is rejected.
- [ ] Stale timestamp is rejected.
- [ ] Duplicate `message.id` creates one job.
- [ ] Duplicate relay job ID produces one send operation.
- [ ] Alarm retries do not duplicate completed jobs.
- [ ] Known retryable failures transition back to `pending`.
- [ ] Ambiguous timeouts transition to `unknown`.
- [ ] Logs contain identifiers and statuses but no private content.
- [ ] Expected failures appear in the typed Effect error channel.
- [ ] Defects remain distinguishable from expected operational failures.

### Real-phone tests

- [ ] Sender A receives the correct text reply.
- [ ] Sender B receives the correct text reply in a separate conversation.
- [x] A delayed message reaches a real phone while the smoke app stays running.
- [ ] A durable delayed message reaches a phone after Worker interruption.
- [x] Inbound attachment bytes were downloaded successfully.
- [ ] An inbound image and PDF are persisted and readable.
- [ ] An outbound image and PDF arrive and open.
- [ ] Disconnecting and restarting the relay recovers message delivery.
- [ ] A failed send retries without duplicate delivery where the transport can
      guarantee idempotency.

## Phase 6: Documentation and task completion

- [ ] Update `README.md` with the final inbound and outbound diagrams.
- [ ] Document local smoke-test, Worker, and relay commands.
- [ ] List environment-variable names without values.
- [ ] Document shared versus dedicated line behavior used by Satchel.
- [ ] Document the Worker/relay runtime boundary.
- [ ] Document delivery limitations, especially the `unknown` send state.
- [ ] Add a dated test-results table with device/sender labels and pass/fail.
- [ ] Attach or link the README and results from the Notion M1 task.
- [ ] Mark the Notion task Done only after every required real-phone test passes.

Effect references:

- [Effect documentation](https://effect.website/docs/)
- [Effect API reference](https://effect-ts.github.io/effect/)
- Vendored Effect 3.22.1 source: `repos/effect`
- Satchel patterns: `agent-patterns/effect.md`

## Pull request sequence

Keep reviews small and reversible:

1. **PR 1 — Worker ingress and architecture:** Worker entry point, signature
   verification, types, Durable Object binding, duplicate fixture test, and
   architecture documentation.
2. **PR 2 — Durable delivery and relay:** scheduled jobs, authenticated relay,
   text sending, restart recovery, and retry-state tests.
3. **PR 3 — File round trip:** R2 persistence, relay streaming, outbound files,
   and file tests.
4. **PR 4 — Proof and documentation:** complete the real-phone matrix, document
   limitations, and attach results to Notion.

## Start here

Do only this next:

> Implement Phase 1 through the health-route checkpoint. Do not add Durable
> Objects, the relay, or R2 until the signed webhook route passes locally.
