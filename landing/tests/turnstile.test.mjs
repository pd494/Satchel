import test from "node:test";
import assert from "node:assert/strict";
import { turnstileConfig, verifyTurnstile } from "../turnstile.js";

const env = {
  TURNSTILE_SITE_KEY: "production-site-key",
  TURNSTILE_SECRET: "test-fixture-secret",
  TURNSTILE_HOSTNAMES: "textsatchel.com,www.textsatchel.com",
  TURNSTILE_ACTION: "signup",
};
const valid = { success: true, action: "signup", hostname: "textsatchel.com" };
const verify = (body, settings = env) => verifyTurnstile("token", "127.0.0.1", settings, "textsatchel.com", async () => Response.json(body));

test("only accepts successful verification for the exact action and hostname", async () => {
  assert.equal(await verify(valid), "valid");
  for (const body of [
    { ...valid, success: false, "error-codes": ["timeout-or-duplicate"] },
    { ...valid, success: "true" },
    { ...valid, action: "login" },
    { ...valid, hostname: "attacker.example" },
    { ...valid, hostname: "www.textsatchel.com" },
    null,
  ]) assert.notEqual(await verify(body), "valid");
});

test("blocks missing, oversized tokens and missing configuration without calling Siteverify", async () => {
  const unexpectedFetch = () => { throw new Error("Must not call Siteverify"); };
  for (const token of [undefined, null, "", 123, "a".repeat(2049)]) {
    assert.equal(await verifyTurnstile(token, "127.0.0.1", env, "textsatchel.com", unexpectedFetch), "invalid");
  }
  assert.equal(await verifyTurnstile("token", "127.0.0.1", {}, "textsatchel.com", unexpectedFetch), "unavailable");
});

test("fails closed on network, HTTP and JSON errors", async () => {
  for (const fetcher of [
    async () => { throw new Error("Network failure"); },
    async () => new Response("upstream failure", { status: 500 }),
    async () => new Response("invalid JSON"),
  ]) assert.equal(await verifyTurnstile("token", "127.0.0.1", env, "textsatchel.com", fetcher), "unavailable");
});

test("production rejects test keys and local hostname allowlists", () => {
  assert.equal(turnstileConfig({ ...env, TURNSTILE_SITE_KEY: "1x00000000000000000000AA" }, "textsatchel.com"), null);
  assert.equal(turnstileConfig({ ...env, TURNSTILE_HOSTNAMES: "textsatchel.com,localhost" }, "textsatchel.com"), null);
  assert.equal(turnstileConfig(env, "localhost"), null);
});

test("Siteverify request uses POST, a timeout, and server-side secret", async () => {
  await verifyTurnstile("fresh-token", "192.0.2.1", env, "textsatchel.com", async (url, options) => {
    assert.equal(url, "https://challenges.cloudflare.com/turnstile/v0/siteverify");
    assert.equal(options.method, "POST");
    assert.equal(options.body.get("secret"), env.TURNSTILE_SECRET);
    assert.equal(options.body.get("response"), "fresh-token");
    assert.equal(options.body.get("remoteip"), "192.0.2.1");
    assert.ok(options.signal instanceof AbortSignal);
    return Response.json(valid);
  });
});
