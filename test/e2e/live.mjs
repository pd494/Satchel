// Eval of Satchel with the real model and a fake Linq. It is not a PR check: the model varies from
// run to run. run.mjs is the PR check. eval.yml runs this one nightly.
//
// Each scenario runs in its own chat, from a new sender. History stays short, and a failure in one
// scenario does not change the next. Each case checks what Satchel sent back to the fake Linq.
// After all scenarios, the run reads each chat's Durable Object: what it saved, its reminders, and
// its time zone. `wrangler dev` needs a Cloudflare login or `CLOUDFLARE_API_TOKEN`.
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
const REPLY_TIMEOUT_MS = 180_000;
const QUIET_MS = 5_000;
const RUN = process.env.EVAL_RUN ?? `local-${new Date().toISOString()}`;
const RUN_STARTED = new Date().toISOString();
const LA = "America/Los_Angeles";

function file(name) {
  const id = randomUUID();
  return { id, url: `https://cdn.linqapp.com/attachments/partners/e2e/${id}/${name}` };
}
const photo = file("photo.jpg");
const memo = file("memo.m4a");
const contact = file("contact.vcf");
const pin = file("CL.loc.vcf");
const lease = file("lease.pdf");

// A case is [name, message, expected kind, needle]. Names are unique across scenarios.
// tapback: a reaction and no text. text-only: text, no reaction, no file.
// text: text, reaction allowed. The needle must be in the text, not only in a link.
// file: the file with the needle as its id is sent back. voice: the same, as a voice memo.
// A null message sends nothing. The case waits for a reminder to fire.
//
// After the scenario, its Durable Object must hold:
// - saved: exactly these links and files, once each.
// - notes: a saved note containing each of these.
// - zone: this time zone, or none.
// - reminders: exactly these pending reminders, one per word. Each is at `hour` in the saved
//   zone, on `weekday` if given, repeating if `repeat`, `days` away if given, and carrying the
//   links in `urls`.
const SCENARIOS = [
  {
    name: "links",
    cases: [
      ["save-link-scheme", "https://www.rust-lang.org/learn", "tapback", ""],
      ["save-link-bare", "example.com/apartments", "tapback", ""],
      ["save-link-with-comment", "check this place out https://www.zillow.com/homes/austin looks nice", "tapback", ""],
      ["save-two-links", "tour at example.org, details at example.org/tour", "tapback", ""],
    ],
    saved: ["https://www.rust-lang.org/learn", "https://example.com/apartments", "https://www.zillow.com/homes/austin", "https://example.org/", "https://example.org/tour"],
  },
  {
    name: "files",
    cases: [
      ["save-image", photo.url, "tapback", ""],
      ["save-voice", memo.url, "tapback", ""],
      ["save-contact", contact.url, "tapback", ""],
      ["save-location", pin.url, "tapback", ""],
      ["save-file", lease.url, "tapback", ""],
      ["recall-image", "send me that picture", "file", photo.id],
      ["recall-voice", "send me that voice memo", "voice", memo.id],
      ["recall-location", "send me that location", "file", pin.id],
      ["recall-file", "send me that pdf", "file", lease.id],
    ],
    saved: [photo.url, memo.url, contact.url, pin.url, lease.url],
  },
  {
    name: "not-saved",
    cases: [
      ["not-a-link", "node.js is my favorite runtime", "text-only", ""],
      ["file-like-not-link", "the readme.md has the setup steps", "text-only", ""],
      ["question-not-saved", "send me the chili recipe", "text-only", ""],
      ["link-question-not-saved", "is this site legit? https://example.net/deal", "text-only", ""],
      ["off-topic", "write me a poem about my cat", "text-only", ""],
      ["small-talk", "ok thanks", "text-only", ""],
    ],
    saved: [],
  },
  {
    name: "notes",
    cases: [
      ["remember-gate", "remember my gate code is 4521", "text-only", ""],
      ["remember-wifi", "the wifi password is sunflower88, don't forget it", "text-only", ""],
      ["recall-gate", "what's my gate code?", "text-only", "4521"],
      ["recall-wifi", "whats the wifi pw again", "text-only", "sunflower88"],
    ],
    saved: [],
    notes: ["4521", "sunflower88"],
  },
  {
    name: "save-and-answer",
    cases: [
      ["save-and-answer", "save https://doc.rust-lang.org/book and tell me what it is", "text", "book"],
      ["recall-after-answer", "what was that book link?", "text-only", ""],
    ],
    saved: ["https://doc.rust-lang.org/book"],
  },
  {
    name: "reminder-fires",
    cases: [
      ["remind-in", "remind me in 2 minutes to drink water", "text-only", ""],
      ["remind-fires", null, "text-only", "water"],
    ],
  },
  {
    name: "zone-then-correct",
    cases: [
      ["remind-asks-zone", "remind me tomorrow at 9 am to stretch", "text-only", "zone"],
      ["remind-set", "pst", "text-only", ""],
      ["remind-correct", "no, make it 10 am", "text-only", ""],
    ],
    zone: LA,
    reminders: [{ word: "stretch", hour: 10 }],
  },
  {
    name: "link-then-correct",
    cases: [
      ["remind-rent", "remind me friday at 3 pm pacific time to pay rent", "text-only", ""],
      ["save-after-reminder", "https://example.com/rent-portal", "tapback", ""],
      ["remind-change-pm", "nvm change it to 5 pm instead", "text-only", ""],
    ],
    saved: ["https://example.com/rent-portal"],
    zone: LA,
    reminders: [{ word: "rent", hour: 17, weekday: "Fri" }],
  },
  {
    name: "link-in-reminder",
    cases: [["remind-with-link", "remind me tomorrow at 9 am pacific time to read this https://example.com/article", "text-only", ""]],
    saved: ["https://example.com/article"],
    zone: LA,
    reminders: [{ word: "read", hour: 9, urls: ["https://example.com/article"] }],
  },
  {
    name: "repeat",
    cases: [["remind-weekly", "every monday at 9 am pacific time remind me to take out the trash", "text-only", ""]],
    zone: LA,
    reminders: [{ word: "trash", hour: 9, weekday: "Mon", repeat: true }],
  },
  {
    name: "push-back",
    cases: [
      ["remind-bank", "remind me tomorrow at 4 pm pacific time to call the bank", "text-only", ""],
      ["remind-push-back", "push it back an hour", "text-only", ""],
    ],
    zone: LA,
    reminders: [{ word: "bank", hour: 17 }],
  },
  {
    name: "change-day",
    cases: [
      ["remind-laundry", "remind me thursday at 6 pm pacific time to pick up laundry", "text-only", ""],
      ["remind-change-day", "actually make it friday", "text-only", ""],
    ],
    zone: LA,
    reminders: [{ word: "laundry", hour: 18, weekday: "Fri" }],
  },
  {
    name: "months-away",
    cases: [["remind-months", "remind me in 3 months to renew my passport", "text-only", ""]],
    reminders: [{ word: "passport", days: [88, 93] }],
  },
  {
    name: "other-zone",
    cases: [
      ["remind-plants", "remind me tomorrow at 8 am pacific time to water the plants", "text-only", ""],
      ["remind-tokyo", "also remind me tomorrow at 9 am tokyo time to call kenji", "text-only", ""],
    ],
    zone: LA,
    // 9 am in Tokyo is 4 pm the day before in Los Angeles.
    reminders: [{ word: "plants", hour: 8 }, { word: "kenji", hour: 16 }],
  },
  {
    name: "moved",
    cases: [
      ["remind-before-move", "remind me tomorrow at 8 am pacific time to feed the cat", "text-only", ""],
      ["remind-after-move", "i just moved to new york. remind me tomorrow at 9 am to call mom", "text-only", ""],
    ],
    // The earlier reminder keeps its time. The new one uses the new zone.
    zone: "America/New_York",
    reminders: [{ word: "cat" }, { word: "mom", hour: 9 }],
  },
];

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

/** Sends `message` from `user` as a signed Linq webhook for case `name`. Returns its event id. */
async function send(user, name, message) {
  const eventId = `eval:${name}:${randomUUID()}`;
  const part = message.startsWith("https://cdn.linqapp.com/")
    ? { type: "media", url: message }
    : { type: "text", value: message };
  const body = JSON.stringify({
    event_type: "message.received",
    data: {
      id: eventId,
      chat: { id: user.chat, is_group: false },
      sender_handle: { handle: user.sender },
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
    const isReminder = eventId === "reminder" && event?.startsWith("reminder:");
    if (event !== null && event !== eventId && !isReminder) {
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

/** Each chat's Durable Object database, found by a message only that chat sent. */
function databaseFor(scenario) {
  const first = scenario.cases.find(([, message]) => message !== null)[1];
  for (const path of readdirSync(storage, { recursive: true })) {
    if (!path.includes("User") || !path.endsWith(".sqlite") || path.endsWith("metadata.sqlite")) continue;
    const db = new DatabaseSync(join(storage, path), { readOnly: true });
    if (db.prepare("SELECT 1 FROM messages WHERE content = ?").get(first)) return db;
    db.close();
  }
  return null;
}

/** Compares a scenario's Durable Object with what it must hold. Returns the failures. */
function checkState(scenario) {
  const db = databaseFor(scenario);
  if (!db) return ["no database for this chat"];
  const saved = db.prepare("SELECT content FROM saved WHERE media != 'note'").all().map((row) => row.content);
  const notes = db.prepare("SELECT content FROM saved WHERE media = 'note'").all().map((row) => row.content);
  const zone = db.prepare("SELECT value FROM settings WHERE key = 'time_zone'").get()?.value ?? null;
  const pending = db.prepare("SELECT id, message_id, content, due_at, repeat FROM saved WHERE media = 'note' AND due_at IS NOT NULL").all();
  const urlsOf = (row) => db.prepare("SELECT content FROM saved WHERE message_id = ? AND media != 'note'").all(row.message_id).map((url) => url.content);

  const failures = [];
  const want = scenario.saved ?? [];
  for (const url of want) {
    const count = saved.filter((content) => content === url).length;
    if (count !== 1) failures.push(`${url} saved ${count} times, not once`);
  }
  for (const content of saved) if (!want.includes(content)) failures.push(`saved by mistake: ${content}`);
  for (const note of scenario.notes ?? []) {
    if (!notes.some((content) => content.includes(note))) failures.push(`no note with "${note}"`);
  }
  if (zone !== (scenario.zone ?? null)) failures.push(`time zone is ${zone}, not ${scenario.zone ?? null}`);

  const reminders = scenario.reminders ?? [];
  if (pending.length !== reminders.length) failures.push(`${pending.length} pending reminders, not ${reminders.length}`);
  const local = (row, options) => new Date(row.due_at * 1000).toLocaleString("en-US", { timeZone: zone ?? "UTC", ...options });
  for (const reminder of reminders) {
    const found = pending.filter((row) => row.content.toLowerCase().includes(reminder.word));
    if (found.length !== 1) {
      failures.push(`expected 1 ${reminder.word} reminder, found ${found.length}`);
      continue;
    }
    const [row] = found;
    const hour = Number(local(row, { hour: "numeric", hourCycle: "h23" }));
    const weekday = local(row, { weekday: "short" });
    const days = (row.due_at * 1000 - Date.now()) / 86_400_000;
    if (reminder.hour !== undefined && hour !== reminder.hour) failures.push(`${reminder.word} reminder is at hour ${hour}, not ${reminder.hour}`);
    if (reminder.weekday && weekday !== reminder.weekday) failures.push(`${reminder.word} reminder is on ${weekday}, not ${reminder.weekday}`);
    if (reminder.repeat && !row.repeat) failures.push(`${reminder.word} reminder does not repeat`);
    if (reminder.days && (days < reminder.days[0] || days > reminder.days[1])) failures.push(`${reminder.word} reminder is ${days.toFixed(1)} days away`);
    for (const url of reminder.urls ?? []) {
      if (!urlsOf(row).includes(url)) failures.push(`${reminder.word} reminder does not carry ${url}`);
    }
  }
  db.close();
  return failures;
}

/** The case that caused a state failure: the case that sent its link, or the scenario's last case. */
function blame(scenario, failure) {
  const url = failure.match(/https:\/\/\S+/)?.[0];
  const bare = url?.replace(/^https:\/\//, "").replace(/\/$/, "");
  const sender = bare && scenario.cases.find(([, message]) => message?.includes(bare));
  return (sender ?? scenario.cases.at(-1))[0];
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
let total = 0;
try {
  await new Promise((resolve) => linq.listen(LINQ_PORT, "127.0.0.1", resolve));
  await waitForWorker();
  for (const scenario of SCENARIOS) {
    const user = { sender: `+1555${Math.floor(Math.random() * 1e7)}`, chat: `eval-${randomUUID()}` };
    console.log(`\n# ${scenario.name}`);
    // The first text in a chat always gets the greeting.
    for (const [name, message, kind, needle] of [[`${scenario.name}-greeting`, "hi", "text", "Satchel"], ...scenario.cases]) {
      const start = linqRequests.length;
      const eventId = message === null ? "reminder" : await send(user, name, message);
      const summary = summarize(await reply(start), eventId);
      const failure = check(kind, needle, summary);
      total += 1;
      if (failure) {
        failures += 1;
        failedCases.add(name);
      }
      console.log(`${failure ? "FAIL" : "PASS"}  ${name}${failure ? ` (${failure})` : ""}\n      ${describe(summary)}`);
    }
  }
  console.log("\n# state");
  for (const scenario of SCENARIOS) {
    const stateFailures = checkState(scenario);
    total += 1;
    for (const failure of stateFailures) {
      console.log(`FAIL  ${scenario.name} state (${failure})`);
      failedCases.add(blame(scenario, failure));
    }
    if (stateFailures.length === 0) console.log(`PASS  ${scenario.name} state`);
    else failures += 1;
  }
  console.log(`\n${total - failures} of ${total} checks passed`);
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
