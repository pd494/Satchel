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

This is a Cloudflare Worker written in Rust with `workers-rs`. The Worker entry
point and HTTP routing live in `src/lib.rs`. Keep it small and avoid speculative
submodules. Everything runs on Cloudflare.

## Working in this project

- Run the app with `npx wrangler dev` (serves on `http://localhost:8787`).
- Check with `cargo fmt --check`,
  `cargo clippy --target wasm32-unknown-unknown -- -D warnings`, and
  `cargo test`.
- The messaging provider is Sendblue; it is not integrated yet.

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
