// What live.mjs, the eval, runs on: a fake Linq, the Worker under `wrangler dev`, signed Linq
// webhooks, and reading what Satchel sent back.

import { spawn } from "node:child_process";
import { createHmac, randomUUID } from "node:crypto";
import { mkdtempSync, readdirSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

const WORKER_PORT = 8787;
const WEBHOOK_KEY = Buffer.from("satchel-e2e-webhook-key");
const BUILD_TIMEOUT_MS = 15 * 60_000;

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** A Linq CDN file with a new id. */
export function file(name) {
  const id = randomUUID();
  return { id, url: `https://cdn.linqapp.com/attachments/partners/e2e/${id}/${name}` };
}

/** Reads a request body as JSON, or null. */
export function readJson(req) {
  return new Promise((resolve) => {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      try {
        resolve(JSON.parse(body));
      } catch {
        resolve(null);
      }
    });
  });
}

/** Starts `server` on a free local port. Returns the port. */
export function listen(server) {
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port)));
}

/** The fake Linq. `requests` holds every request except marking the chat read. Set
 * `refuseNextSend` to answer the next message send with a 500. */
export const linq = {
  requests: [],
  refused: 0,
  refuseNextSend: false,
  server: createServer(async (req, res) => {
    const body = await readJson(req);
    if (linq.refuseNextSend && req.url.endsWith("/messages")) {
      linq.refuseNextSend = false;
      linq.refused += 1;
      return res.writeHead(500).end();
    }
    if (!req.url.endsWith("/read") && !req.url.endsWith("/typing")) linq.requests.push({ path: req.url, body, at: Date.now() });
    res.writeHead(200, { "content-type": "application/json" }).end("{}");
  }),
};

/** Where `wrangler dev` keeps the Durable Objects. */
export const storage = mkdtempSync(join(tmpdir(), "satchel-e2e-"));
let worker;

/** Stops the fake Linq and the Worker. */
export function stop() {
  linq.server.close();
  try {
    process.kill(-worker.pid);
  } catch {}
}
process.on("SIGINT", () => {
  stop();
  process.exit(130);
});
process.on("SIGTERM", () => {
  stop();
  process.exit(143);
});

/** Starts the fake Linq and the Worker, with `args` added to `wrangler dev`. Waits until the Worker
 * answers. */
export async function startWorker(args) {
  const linqPort = await listen(linq.server);
  worker = spawn(
    "npx",
    [
      "wrangler", "dev",
      "--port", String(WORKER_PORT),
      "--persist-to", storage,
      "--var", `LINQ_API:http://127.0.0.1:${linqPort}`,
      "--var", "LINQ_API_KEY:e2e",
      "--var", `LINQ_WEBHOOK_SECRET:whsec_${WEBHOOK_KEY.toString("base64")}`,
      ...args,
    ],
    {
      env: { ...process.env, CLOUDFLARE_LOAD_DEV_VARS_FROM_DOT_ENV: "false" },
      stdio: ["ignore", "inherit", "inherit"],
      detached: true,
    },
  );

  const deadline = Date.now() + BUILD_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (worker.exitCode !== null) throw new Error(`wrangler dev exited with ${worker.exitCode}`);
    try {
      const response = await fetch(`http://127.0.0.1:${WORKER_PORT}/`);
      if ((await response.text()) === "hello world") return;
    } catch {}
    await sleep(2_000);
  }
  throw new Error("wrangler dev did not start");
}

/** A new chat with one sender. Each chat has its own Durable Object. */
export function newUser() {
  return { sender: `+1555${Math.floor(Math.random() * 1e7)}`, chat: `e2e-${randomUUID()}` };
}

/** Posts `message` from `user` as a Linq webhook, signed unless `signature` is given. */
export async function webhook(user, message, { eventId = `e2e-${randomUUID()}`, group = false, signature } = {}) {
  const part = message.startsWith("https://cdn.linqapp.com/")
    ? { type: "media", url: message }
    : { type: "text", value: message };
  const body = JSON.stringify({
    event_type: "message.received",
    data: { id: eventId, chat: { id: user.chat, is_group: group }, sender_handle: { handle: user.sender }, parts: [part] },
  });
  const id = `msg_${randomUUID()}`;
  const timestamp = String(Math.floor(Date.now() / 1000));
  signature ??= `v1,${createHmac("sha256", WEBHOOK_KEY).update(`${id}.${timestamp}.${body}`).digest("base64")}`;
  const response = await fetch(`http://127.0.0.1:${WORKER_PORT}/webhook`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "webhook-id": id,
      "webhook-timestamp": timestamp,
      "webhook-signature": signature,
    },
    body,
  });
  return { eventId, response };
}

/** The event a Linq request replies to. A voice memo request carries none. */
export function eventOf(request) {
  const reaction = request.path.match(/\/messages\/([^/]+)\/reactions$/);
  if (reaction) return reaction[1];
  const key = request.body?.message?.idempotency_key;
  if (key) return key.slice(0, key.lastIndexOf(":"));
  return null;
}

/** Reads the reactions, text, links, and files out of the requests for `eventId`. A reply to
 * another event goes in `late`. */
export function summarize(requests, eventId) {
  const summary = { at: requests[0]?.at, reactions: [], texts: [], links: [], files: [], voices: [], late: [] };
  for (const request of requests) {
    const event = eventOf(request);
    if (event !== null && event !== eventId) {
      summary.late.push(event);
    } else if (request.path.endsWith("/reactions")) {
      summary.reactions.push(request.body.type);
    } else if (request.path.endsWith("/voicememo")) {
      summary.voices.push(request.body.attachment_id);
    } else {
      const part = request.body.message.parts[0];
      if (part.type === "media") summary.files.push(part.attachment_id);
      else if (part.type === "link") summary.links.push(part.value);
      else summary.texts.push(part.value);
    }
  }
  summary.text = summary.texts.join(" ");
  return summary;
}

export function describe(summary) {
  if (!summary) return "(nothing)";
  const items = [];
  for (const reaction of summary.reactions) items.push(`[${reaction}]`);
  for (const text of summary.texts) items.push(JSON.stringify(text));
  for (const link of summary.links) items.push(`link ${link}`);
  for (const id of summary.files) items.push(`file ${id}`);
  for (const id of summary.voices) items.push(`voice ${id}`);
  if (summary.late.length > 0) items.push(`${summary.late.length} late`);
  return items.join(" ") || "(nothing)";
}

/** Paths of the Durable Object databases, one per chat. */
export function userDatabases() {
  return readdirSync(storage, { recursive: true })
    .filter((path) => path.includes("User") && path.endsWith(".sqlite") && !path.endsWith("metadata.sqlite"))
    .map((path) => join(storage, path));
}
