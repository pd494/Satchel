// End-to-end test for Satchel with a fake Linq and a fake model. This is the PR check.
//
// It runs the Worker with `wrangler dev --local`. `LINQ_API` points at a fake Linq, and `AI_API`
// points at a fake model that returns scripted tool calls. Every run is the same.
// Each case checks only what users can see: what Satchel sends to Linq, what it sends to the
// model, and when a reminder fires. live.mjs runs the same Worker with the real model.
//
// Run: node test/e2e/run.mjs

import { spawn } from "node:child_process";
import { createHmac, randomUUID } from "node:crypto";
import { mkdtempSync, readdirSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const WORKER_PORT = 8787;
const WEBHOOK_KEY = Buffer.from("satchel-e2e-webhook-key");
const BUILD_TIMEOUT_MS = 15 * 60_000;
const REPLY_TIMEOUT_MS = 20_000;
const QUIET_MS = 300;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function file(name) {
  const id = randomUUID();
  return { id, url: `https://cdn.linqapp.com/attachments/partners/e2e/${id}/${name}` };
}

/** Reads a request body as JSON. */
function readJson(req) {
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

/** Every request Satchel makes to the fake Linq, except marking the chat read. */
const linqRequests = [];

/** Sends Linq refused. Set `failNextSend` to refuse the next one. */
const failedSends = [];
let failNextSend = false;

const linq = createServer(async (req, res) => {
  const body = await readJson(req);
  if (failNextSend && req.url.endsWith("/messages")) {
    failNextSend = false;
    failedSends.push(body);
    return res.writeHead(500).end();
  }
  if (!req.url.endsWith("/read")) linqRequests.push({ path: req.url, body, at: Date.now() });
  res.writeHead(200, { "content-type": "application/json" }).end("{}");
});

const say = (content) => ({ role: "assistant", content });
const call = (...calls) => ({
  role: "assistant",
  content: "",
  tool_calls: calls.map(([name, args], i) => ({
    id: `call_${i}`,
    type: "function",
    function: { name, arguments: JSON.stringify(args) },
  })),
});

/** The fake model's replies, by user message. Entry n answers step n of the tool loop. An entry
 * can be a function, called when the request arrives. */
const script = new Map();
/** Every request Satchel makes to the fake model. */
const modelRequests = [];

const model = createServer(async (req, res) => {
  const { messages } = await readJson(req);
  const user = messages.findLastIndex((message) => message.role === "user");
  const text = messages[user].content;
  const step = messages.slice(user + 1).filter((message) => message.role === "assistant").length;
  modelRequests.push({ text, messages });

  let reply = script.get(text)?.[step];
  if (typeof reply === "function") reply = await reply();
  // A due reminder comes back as a nudge with its links and files.
  if (text.startsWith("Reminder due: ")) reply = say(`nudge: ${text.slice(14).replaceAll("\n", " ")}`);
  if (!reply) return res.writeHead(500).end(`no scripted reply for ${JSON.stringify(text)} step ${step}`);
  res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ choices: [{ message: reply }] }));
});

/** The tool results Satchel sent the model for its latest answer to `text`. */
function toolResults(text) {
  const request = modelRequests.findLast((request) => request.text === text);
  const user = request.messages.findLastIndex((message) => message.role === "user");
  return request.messages.slice(user + 1).filter((message) => message.role === "tool").map((message) => message.content);
}

function listen(server) {
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port)));
}

const storage = mkdtempSync(join(tmpdir(), "satchel-e2e-"));
let worker;

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

async function startWorker() {
  const linqPort = await listen(linq);
  const modelPort = await listen(model);
  worker = spawn(
    "npx",
    [
      "wrangler", "dev", "--local",
      "--port", String(WORKER_PORT),
      "--persist-to", storage,
      "--var", `LINQ_API:http://127.0.0.1:${linqPort}`,
      "--var", `AI_API:http://127.0.0.1:${modelPort}/`,
      "--var", "LINQ_API_KEY:e2e",
      "--var", `LINQ_WEBHOOK_SECRET:whsec_${WEBHOOK_KEY.toString("base64")}`,
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

const sender = `+1555${Math.floor(Math.random() * 1e7)}`;
const chat = `e2e-${randomUUID()}`;

/** Posts `message` as a Linq webhook, signed unless `signature` is given. */
async function webhook(message, signature, { eventId = `e2e-${randomUUID()}`, group = false } = {}) {
  const part = message.startsWith("https://cdn.linqapp.com/")
    ? { type: "media", url: message }
    : { type: "text", value: message };
  const body = JSON.stringify({
    event_type: "message.received",
    data: { id: eventId, chat: { id: chat, is_group: group }, sender_handle: { handle: sender }, parts: [part] },
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
function eventOf(request) {
  const reaction = request.path.match(/\/messages\/([^/]+)\/reactions$/);
  if (reaction) return reaction[1];
  const key = request.body?.message?.idempotency_key;
  if (key) return key.slice(0, key.lastIndexOf(":"));
  return null;
}

/** Linq requests already checked by a case. */
let checked = 0;

/** Waits up to `timeout` for at least `min` new Linq requests, then until they stop. Returns them. */
async function nextRequests(timeout, min = 1) {
  const deadline = Date.now() + timeout;
  while (linqRequests.length - checked < min && Date.now() < deadline) await sleep(50);
  if (linqRequests.length === checked) return [];
  let count;
  do {
    count = linqRequests.length;
    await sleep(QUIET_MS);
  } while (linqRequests.length !== count);
  const requests = linqRequests.slice(checked);
  checked = linqRequests.length;
  return requests;
}

/** Reads the reactions, text, links, and files out of the requests for `eventId`. */
function summarize(requests, eventId) {
  const summary = { at: requests[0]?.at, reactions: [], texts: [], links: [], files: [], voices: [], late: 0 };
  for (const request of requests) {
    const event = eventOf(request);
    if (event !== null && event !== eventId) {
      summary.late += 1;
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

/** Sends `message` and returns Satchel's reply to it. */
async function turn(message) {
  const { eventId, response } = await webhook(message);
  if (!response.ok) throw new Error(`webhook returned ${response.status}: ${await response.text()}`);
  return summarize(await nextRequests(REPLY_TIMEOUT_MS), eventId);
}

/** Waits up to `timeout` for a reminder nudge. Returns null if none comes. */
async function nudge(timeout) {
  const requests = await nextRequests(timeout);
  if (requests.length === 0) return null;
  const event = eventOf(requests[0]) ?? "";
  return { event, ...summarize(requests, event) };
}

function describe(summary) {
  if (!summary) return "(nothing)";
  const items = [];
  for (const reaction of summary.reactions) items.push(`[${reaction}]`);
  for (const text of summary.texts) items.push(JSON.stringify(text));
  for (const link of summary.links) items.push(`link ${link}`);
  for (const id of summary.files) items.push(`file ${id}`);
  for (const id of summary.voices) items.push(`voice ${id}`);
  if (summary.late > 0) items.push(`${summary.late} late`);
  return items.join(" ") || "(nothing)";
}

/** Formats `ms` as a local time in `zone`, such as 2026-11-23T09:00:00. */
function localTime(ms, zone) {
  const format = new Intl.DateTimeFormat("sv-SE", {
    timeZone: zone, year: "numeric", month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23",
  });
  return format.format(new Date(ms)).replace(" ", "T");
}

const photo = file("photo.jpg");
const memo = file("memo.m4a");
const receipt = file("receipt.jpg");
const listing = "https://example.com/listing";
// The first clock-time reminder saves this zone. Later cases use it.
const ZONE = "Asia/Tokyo";
const SAVED = ["https://example.com/apartments", listing];
const NOT_SAVED = ["https://example.com/unsaved", "https://example.com/wrong"];

// Each case returns a failure, or nothing when it passes. Reminder cases run last, one at a time.
const CASES = [
  ["bad-signature", async () => {
    const { response } = await webhook("hello", "v1,bad");
    if (response.status !== 401) return `status ${response.status}, not 401`;
    if (modelRequests.length > 0) return "called the model";
  }],

  ["greeting", async () => {
    script.set("reset", [say("ignored")]);
    const reply = await turn("reset");
    if (!reply.text.includes("Satchel") || reply.text.includes("ignored")) return `sent ${describe(reply)}`;
  }],

  // A 👍 means saved. It must not go out when the save did not work.
  ["no-tapback-without-save", async () => {
    const message = NOT_SAVED[0];
    script.set(message, [
      call(["react", { emoji: "👍" }]),
      call(["save", { urls: [NOT_SAVED[1]] }], ["react", { emoji: "👍" }]),
      say("could not save"),
    ]);
    const reply = await turn(message);
    if (reply.reactions.length > 0 || reply.text !== "could not save") return `sent ${describe(reply)}`;
    const results = toolResults(message);
    if (results.length !== 3 || !results.every((result) => result.startsWith("error"))) {
      return `the model got ${JSON.stringify(results)}, not 3 errors`;
    }
  }],

  ["tapback-after-save", async () => {
    const message = "example.com/apartments";
    script.set(message, [call(["react", { emoji: "👍" }], ["save", { urls: [message, message] }])]);
    const reply = await turn(message);
    if (reply.reactions.join() !== "like" || reply.texts.length + reply.links.length > 0) return `sent ${describe(reply)}`;
  }],

  // A bad tool call goes back to the model as an error. The reply is cleaned and split into parts.
  ["reply-parts", async () => {
    const message = "send everything";
    script.set(message, [
      call(["remind", { note: "x", after: "PT2S", repeat: "PT1M" }]),
      say(`<thought>secret<channel|>check readme.md https://example.org/book ${photo.url} ${memo.url}<|end|>`),
    ]);
    const reply = await turn(message);
    if (!toolResults(message)[0]?.startsWith("error")) return "the bad tool call got no error";
    const ok = reply.reactions.length === 0
      && reply.text === "check readme.md"
      && reply.links.join() === "https://example.org/book"
      && reply.files.join() === photo.id
      && reply.voices.join() === memo.id;
    if (!ok) return `sent ${describe(reply)}`;
  }],

  // Texts sent fast, without waiting, each get one reply, in order. The model sees the earlier
  // texts and their replies, and never a later text.
  ["burst", async () => {
    const texts = ["first thing", "second thing", "third thing"];
    texts.forEach((text, i) => script.set(text, [say(`reply ${i + 1}`)]));
    // The first answer takes 1 second. The other texts arrive and wait while it runs.
    script.set(texts[0], [async () => (await sleep(1_000), say("reply 1"))]);
    const events = [];
    for (const text of texts) {
      const { eventId, response } = await webhook(text);
      if (!response.ok) return `webhook returned ${response.status}`;
      events.push(eventId);
    }
    const requests = await nextRequests(REPLY_TIMEOUT_MS, texts.length);
    const replies = requests.map((request) => `${eventOf(request)} ${request.body?.message?.parts[0].value}`);
    const expected = events.map((event, i) => `${event} reply ${i + 1}`);
    if (replies.join() !== expected.join()) return `replies were ${JSON.stringify(replies)}`;
    for (const [i, text] of texts.entries()) {
      const seen = modelRequests.findLast((request) => request.text === text).messages.map((message) => message.content);
      if (texts.slice(i + 1).some((later) => seen.includes(later))) return `the model saw a later text while answering "${text}"`;
      if (i > 0 && !seen.includes(`reply ${i}`)) return `the model did not see "reply ${i}" while answering "${text}"`;
    }
  }],

  // Linq sends a webhook again when it gets no answer. The copy gets no second reply.
  ["duplicate-webhook", async () => {
    const message = "same text twice";
    script.set(message, [say("one reply")]);
    const { eventId } = await webhook(message);
    await webhook(message, undefined, { eventId });
    const reply = summarize(await nextRequests(REPLY_TIMEOUT_MS), eventId);
    if (reply.text !== "one reply" || reply.late > 0) return `sent ${describe(reply)}`;
  }],

  // A send that Linq refuses is sent again later, with the stored reply. The model does not run again.
  ["linq-refuses-send", async () => {
    const message = "linq is down";
    script.set(message, [say("stored reply")]);
    failNextSend = true;
    const reply = await turn(message);
    if (failedSends.length !== 1) return `Linq refused ${failedSends.length} sends, not 1`;
    if (reply.text !== "stored reply") return `sent ${describe(reply)}`;
    if (modelRequests.filter((request) => request.text === message).length !== 1) return "the model ran again";
  }],

  // A model that keeps failing gets one fallback reply. The fake model fails on any unscripted text.
  ["model-keeps-failing", async () => {
    const reply = await turn("the model is down");
    if (reply.texts.length !== 1 || reply.reactions.length > 0) return `sent ${describe(reply)}`;
    if ((await nextRequests(3_000)).length > 0) return "sent more than one reply";
  }],

  ["group-chat-ignored", async () => {
    const asked = modelRequests.length;
    const { response } = await webhook("hello everyone", undefined, { group: true });
    if (!response.ok) return `status ${response.status}`;
    if ((await nextRequests(1_500)).length > 0 || modelRequests.length > asked) return "replied to a group chat";
  }],

  ["reminder-fires", async () => {
    script.set(receipt.url, [
      call(["save", { urls: [receipt.url] }], ["remind", { note: "check receipt", after: "PT2S" }]),
      say("set"),
    ]);
    const reply = await turn(receipt.url);
    if (reply.text !== "set" || reply.reactions.length > 0) return `sent ${describe(reply)}`;
    const nudged = await nudge(10_000);
    if (!nudged?.event.startsWith("reminder:") || !nudged.text.includes("check receipt") || nudged.files.join() !== receipt.id) {
      return `nudge was ${describe(nudged)}`;
    }
    if (await nudge(1_500)) return "sent a second nudge";
  }],

  // A clock-time reminder fires at its time. The model is told the zone it saved.
  ["reminder-at-clock-time", async () => {
    const zone = ZONE;
    const message = "remind me to call mom";
    const remind = () => call(["remind", { note: "call mom", at: localTime(Date.now() + 4_000, zone), time_zone: zone }]);
    script.set(message, [remind, remind, say("set")]);
    const reply = await turn(message);
    if (reply.text !== "set") return `sent ${describe(reply)}`;
    const nudged = await nudge(12_000);
    if (!nudged?.text.includes("call mom")) return `nudge was ${describe(nudged)}`;
    const due = modelRequests.findLast((request) => request.text.startsWith("Reminder due: call mom"));
    if (!due.messages[0].content.includes(zone)) return "the model was not told the saved time zone";
  }],
  // A link sent after a reminder is saved on its own. The correction after it moves the reminder
  // to a clock time in the saved zone. The nudge fires once, at the new time, without the link.
  ["reminder-link-then-correct", async () => {
    script.set("remind me to call the landlord", [call(["remind", { note: "call the landlord", after: "PT5S" }]), say("set")]);
    script.set(listing, [call(["save", { urls: [listing] }], ["react", { emoji: "👍" }])]);
    const correction = "nvm change it to 5 pm instead";
    const at = () => localTime(Date.now() + 12_000, ZONE);
    script.set(correction, [() => call(["remind", { note: "Call the landlord", at: at(), time_zone: ZONE }]), say("moved")]);
    if ((await turn("remind me to call the landlord")).text !== "set") return "reminder not set";
    const link = await turn(listing);
    if (link.reactions.join() !== "like" || link.texts.length > 0) return `link got ${describe(link)}`;
    const corrected = Date.now();
    const reply = await turn(correction);
    if (reply.text !== "moved") return `sent ${describe(reply)}`;
    const nudged = await nudge(20_000);
    if (!nudged?.text.includes("call the landlord")) return `nudge was ${describe(nudged)}`;
    if (nudged.links.length + nudged.files.length > 0) return `nudge carried ${describe(nudged)}`;
    const elapsed = nudged.at - corrected;
    if (elapsed < 9_000) return `fired ${elapsed} ms after the correction, before the new time`;
    if (await nudge(1_500)) return "sent a second nudge";
  }],

];

/** Nothing reads `saved` yet, so this reads the table: saved once, and never the unsaved links. */
function checkSaved() {
  const databases = readdirSync(storage, { recursive: true })
    .filter((path) => path.includes("User") && path.endsWith(".sqlite") && !path.endsWith("metadata.sqlite"));
  if (databases.length !== 1) return [`expected 1 User database, found ${databases.length}`];
  const db = new DatabaseSync(join(storage, databases[0]), { readOnly: true });
  const contents = db.prepare("SELECT content FROM saved").all().map((row) => row.content);
  db.close();
  const count = (url) => contents.filter((content) => content === url).length;
  const failures = [];
  for (const url of SAVED) if (count(url) !== 1) failures.push(`${url} saved ${count(url)} times, not once`);
  for (const url of NOT_SAVED) if (count(url) > 0) failures.push(`saved by mistake: ${url}`);
  return failures;
}

let failures = 0;
try {
  await startWorker();
  for (const [name, run] of CASES) {
    const failure = await run();
    if (failure) failures += 1;
    console.log(`${failure ? "FAIL" : "PASS"}  ${name}${failure ? ` (${failure})` : ""}`);
  }
  const savedFailures = checkSaved();
  for (const failure of savedFailures) console.log(`FAIL  saved table (${failure})`);
  if (savedFailures.length === 0) console.log("PASS  saved table");
  failures += savedFailures.length;
  console.log(failures ? `\n${failures} failure(s)` : `\nall ${CASES.length} cases and the saved table passed`);
} catch (error) {
  console.error(error);
  failures += 1;
} finally {
  linq.close();
  model.close();
  stopWorker();
}
process.exit(failures ? 1 : 0);
