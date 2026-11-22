// End-to-end test for Satchel with a fake Linq and the real model. It is not a PR check: the
// model varies from run to run. run.mjs is the PR check. eval.yml runs this one nightly.
//
// It runs the Worker with `wrangler dev`, points `LINQ_API` at a fake Linq server, and sends
// signed webhooks into one chat. Each case checks what Satchel sent back to the fake Linq.
// At the end it reads the Durable Object's `saved` table. The model is the real one.
// `wrangler dev` needs a Cloudflare login or `CLOUDFLARE_API_TOKEN`.
//
// Each model call goes through AI Gateway with the run id in `eval_run` metadata. With
// `CLOUDFLARE_API_TOKEN` (AI Gateway Edit), the run then marks each call 👍 or 👎 by its case.
// To see where the model went wrong: AI Gateway > satchel > Logs, filter Feedback = 👎.
//
// Run: node test/e2e/live.mjs

import { spawn } from "node:child_process";
import { createHmac, randomUUID } from "node:crypto";
import { mkdtempSync, readdirSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const WORKER_PORT = 8787;
const LINQ_PORT = 8788;
const WEBHOOK_KEY = Buffer.from("satchel-e2e-webhook-key");
const BUILD_TIMEOUT_MS = 15 * 60_000;
const REPLY_TIMEOUT_MS = 120_000;
const QUIET_MS = 5_000;
const RUN = process.env.EVAL_RUN ?? `local-${new Date().toISOString()}`;
const RUN_STARTED = new Date().toISOString();

function file(name) {
  const id = randomUUID();
  return { id, url: `https://cdn.linqapp.com/attachments/partners/e2e/${id}/${name}` };
}
const photo = file("photo.jpg");
const memo = file("memo.m4a");
const contact = file("contact.vcf");
const pin = file("CL.loc.vcf");
const lease = file("lease.pdf");

// [name, message, expected kind, needle]
// tapback: a reaction and no text. text-only: text, no reaction, no file.
// text: text, reaction allowed. The needle must be in the text, not only in a link.
// file: the file with the needle as its id is sent back. voice: the same, as a voice memo.
// A null message sends nothing. The case waits for a reminder to fire.
const CASES = [
  ["greeting", "reset", "text", "Satchel"],
  ["save-link-scheme", "https://www.rust-lang.org/learn", "tapback", ""],
  ["save-link-bare", "example.com/apartments", "tapback", ""],
  ["save-link-with-comment", "check this place out https://www.zillow.com/homes/austin looks nice", "tapback", ""],
  ["save-two-links", "tour at example.org, details at example.org/tour", "tapback", ""],
  ["not-a-link", "node.js is my favorite runtime", "text-only", ""],
  ["file-like-not-link", "the readme.md has the setup steps", "text-only", ""],
  ["remember-note", "remember my gate code is 4521", "text-only", ""],
  ["recall-note", "what's my gate code?", "text-only", "4521"],
  ["save-image", photo.url, "tapback", ""],
  ["recall-image", "send me that picture", "file", photo.id],
  ["save-voice", memo.url, "tapback", ""],
  ["recall-voice", "send me that voice memo", "voice", memo.id],
  ["save-contact", contact.url, "tapback", ""],
  ["save-location", pin.url, "tapback", ""],
  ["recall-location", "send me that location", "file", pin.id],
  ["save-file", lease.url, "tapback", ""],
  ["recall-file", "send me that pdf", "file", lease.id],
  ["question-not-saved", "send me the chili recipe", "text-only", ""],
  ["link-question-not-saved", "is this site legit? https://example.net/deal", "text-only", ""],
  ["save-and-answer", "save https://doc.rust-lang.org/book and tell me what it is", "text", "book"],
  ["recall-after-answer", "what was that book link?", "text-only", ""],
  ["remind-in", "remind me in 2 minutes to drink water", "text-only", ""],
  ["remind-asks-zone", "remind me tomorrow at 9 am to stretch", "text-only", "zone"],
  ["remind-set", "pst", "text-only", ""],
  ["remind-correct", "no, make it 10 am", "text-only", ""],
  ["remind-fires", null, "text-only", "water"],
];

// `remind-set` must save this zone.
const TIME_ZONE = "America/Los_Angeles";

// The `saved` table must hold exactly these items, matched in full.
const SAVED = [
  "https://www.rust-lang.org/learn",
  "https://example.com/apartments",
  "https://www.zillow.com/homes/austin",
  "https://example.org/",
  "https://example.org/tour",
  "https://doc.rust-lang.org/book",
  photo.url,
  memo.url,
  contact.url,
  pin.url,
  lease.url,
];
// One saved note must contain this.
const NOTE = "4521";

/** Every request Satchel makes to the fake Linq, except marking the chat read. */
const linqRequests = [];

const linq = createServer((req, res) => {
  let body = "";
  req.on("data", (chunk) => (body += chunk));
  req.on("end", () => {
    if (!req.url.endsWith("/read")) {
      let parsed = null;
      try {
        parsed = JSON.parse(body);
      } catch {}
      linqRequests.push({ path: req.url, body: parsed });
    }
    res.writeHead(200, { "content-type": "application/json" }).end("{}");
  });
});

const storage = mkdtempSync(join(tmpdir(), "satchel-e2e-"));
const worker = spawn(
  "npx",
  [
    "wrangler", "dev",
    "--port", String(WORKER_PORT),
    "--persist-to", storage,
    "--var", `LINQ_API:http://127.0.0.1:${LINQ_PORT}`,
    "--var", "LINQ_API_KEY:e2e",
    "--var", `LINQ_WEBHOOK_SECRET:whsec_${WEBHOOK_KEY.toString("base64")}`,
    "--var", `EVAL_RUN:${RUN}`,
  ],
  {
    env: { ...process.env, CLOUDFLARE_LOAD_DEV_VARS_FROM_DOT_ENV: "false" },
    stdio: ["ignore", "inherit", "inherit"],
    detached: true,
  },
);

function stopWorker() {
  try {
    process.kill(-worker.pid);
  } catch {}
}
process.on("SIGINT", () => {
  stopWorker();
  process.exit(130);
});
process.on("SIGTERM", () => {
  stopWorker();
  process.exit(143);
});

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function waitForWorker() {
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

const sender = `+1555${Math.floor(Math.random() * 1e7)}`;
const chat = `e2e-${randomUUID()}`;

/** Sends `message` as a signed Linq webhook for case `name`. Returns its event id. */
async function send(name, message) {
  const eventId = `eval:${name}:${randomUUID()}`;
  const part = message.startsWith("https://cdn.linqapp.com/")
    ? { type: "media", url: message }
    : { type: "text", value: message };
  const body = JSON.stringify({
    event_type: "message.received",
    data: {
      id: eventId,
      chat: { id: chat, is_group: false },
      sender_handle: { handle: sender },
      parts: [part],
    },
  });
  const id = `msg_${randomUUID()}`;
  const timestamp = String(Math.floor(Date.now() / 1000));
  const signature = createHmac("sha256", WEBHOOK_KEY).update(`${id}.${timestamp}.${body}`).digest("base64");
  const response = await fetch(`http://127.0.0.1:${WORKER_PORT}/webhook`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "webhook-id": id,
      "webhook-timestamp": timestamp,
      "webhook-signature": `v1,${signature}`,
    },
    body,
  });
  if (!response.ok) throw new Error(`webhook returned ${response.status}: ${await response.text()}`);
  return eventId;
}

/** The event a Linq request replies to. A voice memo request carries none. */
function eventOf(request) {
  const reaction = request.path.match(/\/messages\/([^/]+)\/reactions$/);
  if (reaction) return reaction[1];
  const key = request.body?.message?.idempotency_key;
  if (key) return key.slice(0, key.lastIndexOf(":"));
  return null;
}

/** Waits until Satchel stops making Linq requests, and returns the ones after `start`. */
async function reply(start) {
  const deadline = Date.now() + REPLY_TIMEOUT_MS;
  let count = start;
  let quietSince = Date.now();
  while (Date.now() < deadline) {
    await sleep(500);
    if (linqRequests.length !== count) {
      count = linqRequests.length;
      quietSince = Date.now();
    }
    if (count > start && Date.now() - quietSince >= QUIET_MS) break;
  }
  return linqRequests.slice(start);
}

/** Reads the reactions, text, links, and files out of the requests for `eventId`. */
function summarize(requests, eventId) {
  const summary = { reactions: [], texts: [], links: [], files: [], voices: [], late: 0 };
  for (const request of requests) {
    const event = eventOf(request);
    if (event !== null && event !== eventId && !event.startsWith(`${eventId}:`)) {
      summary.late += 1;
      continue;
    }
    if (request.path.endsWith("/reactions")) {
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
  return summary;
}

function check(kind, needle, summary) {
  const text = summary.texts.join(" ");
  const sentFile = summary.files.length + summary.voices.length > 0;
  if (summary.late > 0) return "got a late reply to an earlier message";
  if (kind === "tapback") {
    if (summary.reactions.length === 0) return "no tapback";
    if (text || summary.links.length > 0) return "sent text with the tapback";
  }
  if (kind === "text-only") {
    if (!text && summary.links.length === 0) return "no text";
    if (summary.reactions.length > 0) return "reacted";
    if (sentFile) return "sent a file";
  }
  if (kind === "text" && !text) return "no text";
  if (needle && (kind === "text" || kind === "text-only") && !text.toLowerCase().includes(needle.toLowerCase())) {
    return `text lacks "${needle}"`;
  }
  if (kind === "file" && !summary.files.includes(needle)) return "did not send the saved file";
  if (kind === "voice" && !summary.voices.includes(needle)) return "did not send the saved voice memo";
  return null;
}

function describe(summary) {
  const items = [];
  for (const reaction of summary.reactions) items.push(`[${reaction}]`);
  for (const text of summary.texts) items.push(JSON.stringify(text));
  for (const link of summary.links) items.push(`link ${link}`);
  for (const id of summary.files) items.push(`file ${id}`);
  for (const id of summary.voices) items.push(`voice ${id}`);
  if (summary.late > 0) items.push(`${summary.late} late`);
  return items.join(" ") || "(nothing)";
}

/** Compares the Durable Object's `saved` table with `SAVED` and `NOTE`. Returns the failures. */
function checkSaved() {
  const databases = [];
  for (const path of readdirSync(storage, { recursive: true })) {
    const isUser = path.includes("User") && path.endsWith(".sqlite") && !path.endsWith("metadata.sqlite");
    if (isUser) databases.push(join(storage, path));
  }
  if (databases.length !== 1) return [`expected 1 User database, found ${databases.length}`];
  const db = new DatabaseSync(databases[0], { readOnly: true });
  const rows = db.prepare("SELECT media, content FROM saved").all();
  const zone = db.prepare("SELECT value FROM settings WHERE key = 'time_zone'").get()?.value;
  const pending = db.prepare("SELECT content, due_at FROM saved WHERE media = 'note' AND due_at IS NOT NULL").all();
  db.close();

  const failures = [];
  if (zone !== TIME_ZONE) failures.push(`time zone is ${zone}, not ${TIME_ZONE}`);
  const stretch = pending.filter((row) => row.content.toLowerCase().includes("stretch"));
  const hour = (row) => new Date(row.due_at * 1000).toLocaleString("en-US", { timeZone: TIME_ZONE, hour: "numeric", hourCycle: "h23" });
  if (stretch.length !== 1) failures.push(`expected 1 stretch reminder, found ${stretch.length}`);
  else if (hour(stretch[0]) !== "10") failures.push(`stretch reminder is at hour ${hour(stretch[0])}, not 10`);
  const contents = [];
  let noteFound = false;
  for (const row of rows) {
    if (row.media === "note") {
      if (row.content.includes(NOTE)) noteFound = true;
    } else {
      contents.push(row.content);
    }
  }
  if (!noteFound) failures.push(`no note with "${NOTE}"`);
  for (const item of SAVED) {
    if (!contents.includes(item)) failures.push(`not saved: ${item}`);
  }
  for (const content of contents) {
    if (!SAVED.includes(content)) failures.push(`saved by mistake: ${content}`);
  }
  return failures;
}

/** The case that caused a saved table failure. */
function blame(failure) {
  if (failure.includes("stretch")) return "remind-correct";
  if (failure.includes("time zone")) return "remind-set";
  if (failure.includes(NOTE)) return "remember-note";
  const url = failure.slice(failure.indexOf(": ") + 2);
  const bare = url.replace(/^https:\/\//, "").replace(/\/$/, "");
  return CASES.find(([, message]) => message?.includes(bare))?.[0] ?? "unknown";
}

/** Marks each AI Gateway log from this run 👍 or 👎 by its case. A due reminder belongs to `remind-fires`. */
async function markLogs() {
  const token = process.env.CLOUDFLARE_API_TOKEN;
  if (!token) return console.log("no CLOUDFLARE_API_TOKEN: AI Gateway logs not marked");
  const account = readFileSync(new URL("../../wrangler.jsonc", import.meta.url), "utf8").match(/"account_id":\s*"(\w+)"/)[1];
  const api = `https://api.cloudflare.com/client/v4/accounts/${account}/ai-gateway/gateways/satchel/logs`;
  const headers = { authorization: `Bearer ${token}`, "content-type": "application/json" };
  await sleep(15_000); // Logs take a few seconds to show up.

  const logs = [];
  for (let page = 1; ; page++) {
    const url = `${api}?start_date=${RUN_STARTED}&per_page=50&page=${page}&order_by=created_at&order_by_direction=asc`;
    const response = await fetch(url, { headers });
    const body = await response.json();
    if (!body.success) return console.log(`AI Gateway logs not marked: ${JSON.stringify(body.errors)}`);
    for (const log of body.result) {
      const metadata = JSON.parse(log.metadata || "{}");
      if (metadata.eval_run === RUN) logs.push({ id: log.id, event: String(metadata.event_id) });
    }
    if (body.result.length < 50) break;
  }

  let marked = 0;
  for (const { id, event } of logs) {
    const name = event.startsWith("reminder:") ? "remind-fires" : event.split(":")[1];
    const feedback = failedCases.has(name) ? -1 : 1;
    const response = await fetch(`${api}/${id}`, { method: "PATCH", headers, body: JSON.stringify({ feedback, metadata: { eval_run: RUN, case: name } }) });
    if (response.ok) marked += 1;
  }
  console.log(`marked ${marked} of ${logs.length} AI Gateway logs for run ${RUN} (${failedCases.size} failed cases)`);
}

const failedCases = new Set();
let failures = 0;
try {
  await new Promise((resolve) => linq.listen(LINQ_PORT, "127.0.0.1", resolve));
  await waitForWorker();
  for (const [name, message, kind, needle] of CASES) {
    const start = linqRequests.length;
    const eventId = message === null ? "reminder" : await send(name, message);
    const summary = summarize(await reply(start), eventId);
    const failure = check(kind, needle, summary);
    if (failure) failures += 1;
    if (failure) failedCases.add(name);
    console.log(`${failure ? "FAIL" : "PASS"}  ${name}${failure ? ` (${failure})` : ""}\n      ${describe(summary)}`);
  }
  const savedFailures = checkSaved();
  for (const failure of savedFailures) {
    console.log(`FAIL  saved table (${failure})`);
    failedCases.add(blame(failure));
  }
  if (savedFailures.length === 0) console.log(`PASS  saved table (${SAVED.length} items, the note, and the time zone)`);
  failures += savedFailures.length;
  console.log(failures ? `\n${failures} failure(s)` : `\nall ${CASES.length} cases and the saved table passed`);
} catch (error) {
  console.error(error);
  failures += 1;
} finally {
  linq.close();
  stopWorker();
}
try {
  await markLogs();
} catch (error) {
  console.log(`AI Gateway logs not marked: ${error}`);
}
process.exit(failures ? 1 : 0);
