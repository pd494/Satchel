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

## Project

Satchel is an iMessage assistant for saving things and finding them later.
Users text a Sendblue number with links, photos, notes, or questions ("what was
that apartment place I saved?"), and Satchel saves, searches, and remembers.

How it works:

- Sendblue POSTs each inbound message to a Cloudflare Worker written in Rust
  with `workers-rs`. The entry point and HTTP routing live in `src/lib.rs`.
- The Worker returns 200 to Sendblue immediately and hands the message to that
  phone number's Durable Object via `wait_until`. All of a user's data lives
  in that object's SQLite storage.
- No alarms or schedulers. All work runs inside the request a message
  triggers: a ~3 s timer debounces bursts into one turn, follow-up work runs
  right after the reply, and anything that fails is flagged and retried after
  the user's next message.
- Inside the Durable Object, an agent harness batches messages into turns,
  builds context, asks Workers AI for exactly one typed `Plan` per turn,
  executes it in code, and replies through Sendblue. The model decides; code
  acts and writes every reply that shows user data.
- Storage: Durable Object SQLite (source of truth, FTS5 keyword search), R2
  (media), Vectorize (semantic search). Memory beyond the recent conversation
  (facts, profile, recap) is written after the reply or when the user texts
  again after a 30+ minute gap, never by the model's per-turn decision.
- Everything runs on Cloudflare's free plan.

Keep the code small and avoid speculative submodules.

## Goals

- Save anything texted (links, images, notes, files) with nothing lost, even
  when AI is unavailable.
- Find it later from vague, natural questions, including follow-ups like
  "#2" or "older".
- Remember preferences and facts the user states, with dated corrections.
- Stay quiet on noise (acks, reactions) and decline off-topic requests.
- Keep each user's data isolated, deletable, and exportable.
- Ship as small, self-contained, verifiable PRs, tracked by milestone on the
  Satchel Notion board (each card has goal, build, and verify steps).

Non-goals: general-purpose assistant tasks (reminders, messaging others,
answering general-knowledge questions) and group chats.

## Working in this project

- Run the app with `npx wrangler dev` (serves on `http://localhost:8787`).
- Check with `cargo fmt --check`,
  `cargo clippy --target wasm32-unknown-unknown -- -D warnings`, and
  `cargo test`.
- The messaging provider is Sendblue; it is not integrated yet.
- Default model: `@cf/meta/llama-4-scout-17b-16e-instruct`. The free Workers AI
  budget is 10,000 neurons/day, so keep prompts small.

## Rust architecture

- Keep the crate lints in `Cargo.toml` enabled; do not `#[allow]` around them.
- Parse unknown external input at its boundary with `serde` and pass refined
  values inward.
- Represent expected failures as precise `thiserror` enums in `Result`. Reserve
  panics for defects, and translate external errors (including JS rejections)
  at the adapter that owns them.
- Keep application operations independent of Cloudflare, SQL, HTTP, and SDK
  types by depending on narrow application-owned traits.
- Keep Cloudflare bindings, concrete adapters, secret unwrapping, and structured
  logging in the Worker composition root.
- Use newtypes where mixing identifiers would be a realistic mistake.
- Cloudflare durable primitives remain responsible for persistence and
  idempotency across invocations.

## Environment

This project reads secrets from `.env` (gitignored). **Do not read, write, or echo `.env`** — it contains credentials.
