// Eval of Satchel with the real model and a fake Linq. It is not a PR check: the model varies from
// run to run. run.mjs is the PR check. eval.yml runs this one nightly.
//
// Each scenario runs in its own chat, from a new sender. History stays short, and a failure in one
// scenario does not change the next. Each case checks what Satchel sent back to the fake Linq and,
// where it matters, what the chat's Durable Object holds after the turn. After all its cases, each
// scenario checks what the Durable Object saved, its notes, its time zone, and its reminders.
// `wrangler dev` needs a Cloudflare login or `CLOUDFLARE_API_TOKEN`.
//
// Results: with `CLOUDFLARE_API_TOKEN` (AI Gateway Edit, D1 Edit), the run marks each AI Gateway
// log 👍 or 👎 by its case, and writes one row per case to the `satchel-evals` D1 database.
// - Pass rates: D1 > satchel-evals > Console, `SELECT * FROM pass_rates`.
// - A failure's full context: copy its `log_id` from `SELECT * FROM failures`, then open
//   AI Gateway > satchel > Logs and filter by Log ID.
//
// Run: node test/e2e/live.mjs

import { execSync, spawn } from "node:child_process";
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
const COMMIT = process.env.GITHUB_SHA ?? execSync("git rev-parse HEAD").toString().trim();
const MODEL = readFileSync(new URL("../../src/agent.rs", import.meta.url), "utf8").match(/const MODEL: &str = "([^"]+)"/)[1];
const ACCOUNT = readFileSync(new URL("../../wrangler.jsonc", import.meta.url), "utf8").match(/"account_id":\s*"(\w+)"/)[1];
const EVAL_DB = "d3b42584-e526-46b4-8d38-f51fe8e2d3f9";
const LA = "America/Los_Angeles";

function file(name) {
  const id = randomUUID();
  return { id, url: `https://cdn.linqapp.com/attachments/partners/e2e/${id}/${name}` };
}
const photo = file("photo.jpg");
const memo = file("memo.m4a");
const pin = file("CL.loc.vcf");
const lease = file("lease.pdf");
const BOOK = "https://doc.rust-lang.org/book";
const WATER = "https://example.com/water-log";

// A case is [name, message, expected kind, needle, after]. Names are unique across scenarios.
// tapback: a reaction and no text. text-only: text, no reaction, no file.
// file: the file with the needle as its id is sent back. voice: the same, as a voice memo.
// link: the needle is sent as a link.
// A null message sends nothing. The case waits for a reminder to fire.
// after: what the chat's Durable Object holds after the turn: `pending` reminders, or a `saved` URL.
//
// After all its cases, a scenario's Durable Object must hold:
// - saved: exactly these links and files, once each.
// - notes: exactly one note per entry, containing it. Checked only in scenarios without reminders.
// - zone: a time zone with the same UTC offsets as this one, or none.
// - reminders: exactly these pending reminders, one per word. Each is at `hour` in the saved zone
//   (or in `tz`), on `weekday` if given, weekly if `weekly`, `days` away if given, and carrying the
//   links in `urls`.
// In a `chain` scenario, each turn depends on the last. After a failure, later failures are
// `blocked`, not counted.
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
      ["save-location", pin.url, "tapback", ""],
      ["save-file", lease.url, "tapback", ""],
      ["recall-image", "send me that picture", "file", photo.id],
      ["recall-voice", "send me that voice memo", "voice", memo.id],
      ["recall-location", "send me that location", "file", pin.id],
      ["recall-file", "send me that pdf", "file", lease.id],
    ],
    saved: [photo.url, memo.url, pin.url, lease.url],
  },
  {
    name: "not-saved",
    cases: [
      ["not-a-link", "node.js is my favorite runtime", "text-only", ""],
      ["file-like-not-link", "the readme.md has the setup steps", "text-only", ""],
      ["question-not-saved", "send me the chili recipe", "text-only", ""],
      ["link-question-not-saved", "is this site legit? https://example.net/deal", "text-only", ""],
      ["small-talk", "ok thanks", "text-only", ""],
    ],
  },
  {
    name: "notes",
    cases: [
      ["remember-gate", "remember my gate code is 4521", "text-only", ""],
      ["remember-wifi", "the wifi password is sunflower88, don't forget it", "text-only", ""],
      ["recall-gate", "what's my gate code?", "text-only", "4521"],
      ["recall-wifi", "whats the wifi pw again", "text-only", "sunflower88"],
    ],
    notes: ["4521", "sunflower88"],
  },
  {
    name: "save-and-answer",
    chain: true,
    cases: [
      ["save-and-answer", `save ${BOOK} and tell me what it is`, "text-only", "", { saved: BOOK }],
      ["recall-after-answer", "what was that book link?", "link", BOOK],
    ],
    saved: [BOOK],
  },
  {
    name: "reminder-fires",
    chain: true,
    cases: [
      ["remind-in", `remind me in 2 minutes to drink water and log it here ${WATER}`, "text-only", "", { pending: 1 }],
      ["remind-fires", null, "link", WATER],
    ],
    saved: [WATER],
  },
  {
    name: "zone-then-correct",
    chain: true,
    cases: [
      ["remind-asks-zone", "remind me tomorrow at 9 am to stretch", "text-only", "", { pending: 0 }],
      ["remind-set", "pst", "text-only", "", { pending: 1 }],
      ["remind-correct", "no, make it 10 am", "text-only", ""],
    ],
    zone: LA,
    reminders: [{ word: "stretch", hour: 10 }],
  },
  {
    name: "link-then-correct",
    chain: true,
    cases: [
      ["remind-rent", "remind me friday at 3 pm pacific time to pay rent", "text-only", "", { pending: 1 }],
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
    reminders: [{ word: "trash", hour: 9, weekday: "Mon", weekly: true }],
  },
  {
    name: "push-back",
    chain: true,
    cases: [
      ["remind-bank", "remind me tomorrow at 4 pm pacific time to call the bank", "text-only", "", { pending: 1 }],
      ["remind-push-back", "push it back an hour", "text-only", ""],
    ],
    zone: LA,
    reminders: [{ word: "bank", hour: 17 }],
  },
  {
    name: "change-day",
    chain: true,
    cases: [
      ["remind-laundry", "remind me thursday at 6 pm pacific time to pick up laundry", "text-only", "", { pending: 1 }],
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
    chain: true,
    cases: [
      ["remind-plants", "remind me tomorrow at 8 am pacific time to water the plants", "text-only", "", { pending: 1 }],
      ["remind-tokyo", "also remind me tomorrow at 9 am tokyo time to call kenji", "text-only", ""],
    ],
    zone: LA,
    reminders: [{ word: "plants", hour: 8 }, { word: "kenji", hour: 9, tz: "Asia/Tokyo" }],
  },
  {
    name: "moved",
    chain: true,
    cases: [
      ["remind-before-move", "remind me tomorrow at 8 am pacific time to feed the cat", "text-only", "", { pending: 1 }],
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

/** Reads the reactions, text, links, and files out of the requests for `eventId`. A reply to
 * another event goes in `late`, by event. */
function summarize(requests, eventId) {
  const summary = { reactions: [], texts: [], links: [], files: [], voices: [], late: [] };
  for (const request of requests) {
    const event = eventOf(request);
    const isNudge = eventId === "reminder" && event?.startsWith("reminder:");
    if (event !== null && event !== eventId && !isNudge) {
      summary.late.push(event);
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
  if (kind === "tapback") {
    if (summary.reactions.length === 0) return "no tapback";
    if (text || summary.links.length > 0) return "sent text with the tapback";
  }
  if (kind === "text-only") {
    if (!text && summary.links.length === 0) return "no text";
    if (summary.reactions.length > 0) return "reacted";
    if (sentFile) return "sent a file";
  }
  if (needle && kind === "text-only" && !text.toLowerCase().includes(needle.toLowerCase())) return `text lacks "${needle}"`;
  if (kind === "file" && !summary.files.includes(needle)) return "did not send the saved file";
  if (kind === "voice" && !summary.voices.includes(needle)) return "did not send the saved voice memo";
  if (kind === "link" && !summary.links.includes(needle)) return `did not send ${needle}`;
  return null;
}

function describe(summary) {
  const items = [];
  for (const reaction of summary.reactions) items.push(`[${reaction}]`);
  for (const text of summary.texts) items.push(JSON.stringify(text));
  for (const link of summary.links) items.push(`link ${link}`);
  for (const id of summary.files) items.push(`file ${id}`);
  for (const id of summary.voices) items.push(`voice ${id}`);
  return items.join(" ") || "(nothing)";
}

/** Opens the Durable Object database of the chat that sent `text`. */
function databaseWith(text) {
  for (const path of readdirSync(storage, { recursive: true })) {
    if (!path.includes("User") || !path.endsWith(".sqlite") || path.endsWith("metadata.sqlite")) continue;
    const db = new DatabaseSync(join(storage, path), { readOnly: true });
    if (db.prepare("SELECT 1 FROM messages WHERE content = ?").get(text)) return db;
    db.close();
  }
  return null;
}

/** Checks what the chat's Durable Object holds right after a turn. */
function checkAfter(after, message) {
  if (!after) return null;
  const db = databaseWith(message);
  if (!db) return "no database for this chat";
  const pending = db.prepare("SELECT count(*) AS n FROM saved WHERE media = 'note' AND due_at IS NOT NULL").get().n;
  const saved = db.prepare("SELECT content FROM saved WHERE media != 'note'").all().map((row) => row.content);
  db.close();
  if (after.pending !== undefined && pending !== after.pending) return `${pending} pending reminders after this turn, not ${after.pending}`;
  if (after.saved && !saved.includes(after.saved)) return `did not save ${after.saved}`;
  return null;
}

/** A zone's UTC offsets in January and July. Two names for the same zone match. */
function offsets(zone) {
  if (!zone) return "none";
  return ["2026-01-15", "2026-07-15"].map((day) => new Date(day).toLocaleString("en-US", { timeZone: zone, timeZoneName: "longOffset" }).split(" ").pop()).join();
}

/** Compares a scenario's Durable Object with what it must hold. Returns [failure, case to blame]. */
function checkState(scenario) {
  const first = scenario.cases.find(([, message]) => message !== null)[1];
  const last = scenario.cases.at(-1)[0];
  const db = databaseWith(first);
  if (!db) return [["no database for this chat", last]];
  const saved = db.prepare("SELECT content FROM saved WHERE media != 'note'").all().map((row) => row.content);
  const notes = db.prepare("SELECT content FROM saved WHERE media = 'note'").all().map((row) => row.content);
  const zone = db.prepare("SELECT value FROM settings WHERE key = 'time_zone'").get()?.value ?? null;
  const pending = db.prepare("SELECT message_id, content, due_at, repeat FROM saved WHERE media = 'note' AND due_at IS NOT NULL").all();
  const urlsOf = (row) => db.prepare("SELECT content FROM saved WHERE message_id = ? AND media != 'note'").all(row.message_id).map((url) => url.content);
  const sentBy = (text) => scenario.cases.find(([, message]) => message?.includes(text.replace(/^https:\/\//, "").replace(/\/$/, "")))?.[0] ?? last;

  const failures = [];
  const want = scenario.saved ?? [];
  for (const url of want) {
    const count = saved.filter((content) => content === url).length;
    if (count !== 1) failures.push([`${url} saved ${count} times, not once`, sentBy(url)]);
  }
  for (const content of saved) if (!want.includes(content)) failures.push([`saved by mistake: ${content}`, sentBy(content)]);
  if (!scenario.reminders) {
    const wantNotes = scenario.notes ?? [];
    for (const note of wantNotes) {
      const count = notes.filter((content) => content.includes(note)).length;
      if (count !== 1) failures.push([`${count} notes with "${note}", not 1`, sentBy(note)]);
    }
    if (notes.length > wantNotes.length) failures.push([`${notes.length} notes, not ${wantNotes.length}`, last]);
  }
  if (offsets(zone) !== offsets(scenario.zone)) failures.push([`time zone is ${zone}, not ${scenario.zone ?? null}`, last]);

  const reminders = scenario.reminders ?? [];
  if (pending.length !== reminders.length) failures.push([`${pending.length} pending reminders, not ${reminders.length}`, last]);
  for (const reminder of reminders) {
    const found = pending.filter((row) => row.content.toLowerCase().includes(reminder.word));
    if (found.length !== 1) {
      failures.push([`expected 1 ${reminder.word} reminder, found ${found.length}`, last]);
      continue;
    }
    const [row] = found;
    const local = (options) => new Date(row.due_at * 1000).toLocaleString("en-US", { timeZone: reminder.tz ?? zone ?? "UTC", ...options });
    const hour = Number(local({ hour: "numeric", hourCycle: "h23" }));
    const weekday = local({ weekday: "short" });
    const days = (row.due_at * 1000 - Date.now()) / 86_400_000;
    const problems = [
      reminder.hour !== undefined && hour !== reminder.hour && `is at hour ${hour}, not ${reminder.hour}`,
      reminder.weekday && weekday !== reminder.weekday && `is on ${weekday}, not ${reminder.weekday}`,
      reminder.weekly && !/^P(1W|7D)$/i.test(row.repeat ?? "") && `repeats ${row.repeat ?? "never"}, not weekly`,
      reminder.days && (days < reminder.days[0] || days > reminder.days[1]) && `is ${days.toFixed(1)} days away`,
      ...(reminder.urls ?? []).filter((url) => !urlsOf(row).includes(url)).map((url) => `does not carry ${url}`),
    ];
    for (const problem of problems.filter(Boolean)) failures.push([`${reminder.word} reminder ${problem}`, last]);
  }
  db.close();
  return failures;
}

/** One result per case: its scenario, verdict, reason, reply, and the event it sent. */
const results = new Map();

function fail(name, reason, blocked) {
  const result = results.get(name);
  if (result.verdict !== "pass") return;
  Object.assign(result, { verdict: blocked ? "blocked" : "fail", reason });
}

/** Lists this run's AI Gateway logs, marks each 👍 or 👎 by its case, and returns each case's last
 * log id. A due reminder belongs to `remind-fires`. */
async function markLogs(api, headers) {
  await sleep(15_000); // Logs take a few seconds to show up.
  const logs = [];
  for (let page = 1; ; page++) {
    const url = `${api}/ai-gateway/gateways/satchel/logs?start_date=${RUN_STARTED}&per_page=50&page=${page}&order_by=created_at&order_by_direction=asc`;
    const body = await (await fetch(url, { headers })).json();
    if (!body.success) throw new Error(`AI Gateway logs: ${JSON.stringify(body.errors)}`);
    for (const log of body.result) {
      const metadata = typeof log.metadata === "string" ? JSON.parse(log.metadata || "{}") : (log.metadata ?? {});
      if (metadata.eval_run !== RUN) continue;
      const event = String(metadata.event_id);
      logs.push({ id: log.id, name: event.startsWith("reminder:") ? "remind-fires" : event.split(":")[1] });
    }
    if (body.result.length < 50) break;
  }
  const lastLog = new Map();
  for (const { id, name } of logs) {
    lastLog.set(name, id);
    const verdict = results.get(name)?.verdict;
    const feedback = verdict === "fail" ? -1 : verdict === "pass" ? 1 : 0;
    await fetch(`${api}/ai-gateway/gateways/satchel/logs/${id}`, { method: "PATCH", headers, body: JSON.stringify({ feedback }) });
  }
  console.log(`marked ${logs.length} AI Gateway logs for run ${RUN}`);
  return lastLog;
}

/** Writes one row per case to the `satchel-evals` D1 database. */
async function writeResults(api, headers, lastLog) {
  const rows = [...results].map(([name, result]) => [RUN, RUN_STARTED, COMMIT, MODEL, result.scenario, name, result.verdict, result.reason, result.reply, lastLog.get(name) ?? null]);
  for (let i = 0; i < rows.length; i += 10) {
    const chunk = rows.slice(i, i + 10);
    const sql = `INSERT OR REPLACE INTO results VALUES ${chunk.map(() => "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)").join(", ")}`;
    const body = await (await fetch(`${api}/d1/database/${EVAL_DB}/query`, { method: "POST", headers, body: JSON.stringify({ sql, params: chunk.flat() }) })).json();
    if (!body.success) throw new Error(`D1: ${JSON.stringify(body.errors)}`);
  }
  console.log(`wrote ${rows.length} results to D1 satchel-evals`);
}

let crashed = false;
try {
  await new Promise((resolve) => linq.listen(LINQ_PORT, "127.0.0.1", resolve));
  await waitForWorker();
  for (const scenario of SCENARIOS) {
    const user = { sender: `+1555${Math.floor(Math.random() * 1e7)}`, chat: `eval-${randomUUID()}` };
    const events = new Map();
    console.log(`\n# ${scenario.name}`);
    // The first text in a chat always gets the greeting, whatever the model says. It is not a check.
    const greeted = linqRequests.length;
    await send(user, `${scenario.name}-greeting`, "hi");
    await reply(greeted);
    let broken = false;
    for (const [name, message, kind, needle, after] of scenario.cases) {
      results.set(name, { scenario: scenario.name, verdict: "pass", reason: null, reply: null });
      const start = linqRequests.length;
      const eventId = message === null ? "reminder" : await send(user, name, message);
      events.set(eventId, name);
      const summary = summarize(await reply(start), eventId);
      results.get(name).reply = describe(summary);
      for (const event of summary.late) {
        if (events.has(event)) fail(events.get(event), "replied again after the turn ended", false);
      }
      const reason = check(kind, needle, summary) ?? checkAfter(after, message);
      if (reason) fail(name, reason, scenario.chain && broken);
      broken ||= Boolean(reason);
      const { verdict } = results.get(name);
      console.log(`${verdict.toUpperCase().padEnd(7)} ${name}${reason ? ` (${reason})` : ""}\n        ${describe(summary)}`);
    }
    for (const [reason, name] of checkState(scenario)) {
      const blocked = scenario.chain && scenario.cases.some(([other]) => other !== name && results.get(other).verdict !== "pass");
      fail(name, reason, blocked);
      console.log(`${(blocked ? "BLOCKED" : "FAIL").padEnd(7)} ${name} (${reason})`);
    }
  }
  const count = (verdict) => [...results.values()].filter((result) => result.verdict === verdict).length;
  console.log(`\n${count("pass")} passed, ${count("fail")} failed, ${count("blocked")} blocked, of ${results.size} cases`);
} catch (error) {
  console.error(error);
  crashed = true;
} finally {
  linq.close();
  stopWorker();
}

const token = process.env.CLOUDFLARE_API_TOKEN;
if (!token) {
  console.log("no CLOUDFLARE_API_TOKEN: results not sent to AI Gateway or D1");
} else if (results.size > 0) {
  const api = `https://api.cloudflare.com/client/v4/accounts/${ACCOUNT}`;
  const headers = { authorization: `Bearer ${token}`, "content-type": "application/json" };
  try {
    await writeResults(api, headers, await markLogs(api, headers));
  } catch (error) {
    console.error(`results not sent: ${error.message}`);
    crashed = true;
  }
}
// A model failure is a result, not a broken run. The run fails only when it could not finish.
process.exit(crashed ? 1 : 0);
