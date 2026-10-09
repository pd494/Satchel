import { DurableObject } from "cloudflare:workers";
import { normalizeEmail } from "./email.js";
import { turnstileConfig, verifyTurnstile } from "./turnstile.js";

const MAX_BODY_BYTES = 4096;

function reply(body, status = 200, headers = {}) {
  return Response.json(body, {
    status,
    headers: { "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff", ...headers },
  });
}

// Count streamed bytes; Content-Length can be absent or wrong.
async function readJson(request) {
  if (!request.body) return { error: "invalid_signup", status: 400 };
  const reader = request.body.getReader();
  const chunks = [];
  let length = 0;
  try {
    for (let r = await reader.read(); !r.done; r = await reader.read()) {
      length += r.value.byteLength;
      if (length > MAX_BODY_BYTES) {
        await reader.cancel();
        return { error: "request_too_large", status: 413 };
      }
      chunks.push(r.value);
    }
    const bytes = await new Blob(chunks).arrayBuffer();
    return { json: JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)) };
  } catch {
    return { error: "invalid_signup", status: 400 };
  }
}

export class Waitlist extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    ctx.blockConcurrencyWhile(async () => {
      ctx.storage.sql.exec(`CREATE TABLE IF NOT EXISTS signups (
        email TEXT PRIMARY KEY,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
      )`);
    });
  }

  // Returns false if the email was already on the list.
  signup(email) {
    return this.ctx.storage.sql.exec(
      "INSERT INTO signups (email) VALUES (?) ON CONFLICT(email) DO NOTHING RETURNING email",
      email,
    ).toArray().length === 1;
  }
}

async function handleSignup(request, env, url) {
  if (request.method !== "POST") return reply({ error: "method_not_allowed" }, 405, { Allow: "POST" });
  const origin = request.headers.get("Origin");
  if ((origin && origin !== url.origin) || request.headers.get("Sec-Fetch-Site") === "cross-site") {
    return reply({ error: "forbidden" }, 403);
  }
  if (request.headers.get("Content-Type")?.split(";")[0].trim().toLowerCase() !== "application/json") {
    return reply({ error: "unsupported_content_type" }, 415);
  }
  if (Number(request.headers.get("Content-Length")) > MAX_BODY_BYTES) {
    return reply({ error: "request_too_large" }, 413);
  }

  // Cloudflare sets this at the edge; X-Forwarded-For is client-controlled.
  const ip = request.headers.get("CF-Connecting-IP") ?? "unknown";
  const allowed = (await env.SIGNUP_IP_LIMIT.limit({ key: `waitlist:${ip}` })).success &&
    (await env.SIGNUP_TOTAL_LIMIT.limit({ key: "waitlist:total" })).success;
  if (!allowed) return reply({ error: "rate_limited" }, 429, { "Retry-After": "60" });

  const { json, error, status } = await readJson(request);
  if (error) return reply({ error }, status);
  const email = normalizeEmail(json?.email);
  if (!email) return reply({ error: "invalid_email" }, 400);

  const verification = await verifyTurnstile(json?.token, ip, env, url.hostname);
  if (verification === "unavailable") return reply({ error: "temporarily_unavailable" }, 503);
  if (verification !== "valid") return reply({ error: "verification_failed" }, 403);

  const added = await env.WAITLIST.getByName("signups").signup(email);
  return added ? reply({ ok: true }) : reply({ error: "already_registered" }, 409);
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    try {
      if (url.pathname === "/api/waitlist") return await handleSignup(request, env, url);
      if (url.pathname === "/api/turnstile" && request.method === "GET") {
        const config = turnstileConfig(env, url.hostname);
        return config
          ? reply({ sitekey: config.sitekey, action: config.action })
          : reply({ error: "temporarily_unavailable" }, 503);
      }
      if (url.pathname.startsWith("/api/")) return reply({ error: "not_found" }, 404);
      return env.ASSETS.fetch(request);
    } catch (err) {
      console.error({ event: "request_failed", path: url.pathname, error: String(err) });
      return reply({ error: "temporarily_unavailable" }, 503);
    }
  },
};
