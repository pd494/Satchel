// End-to-end test for Satchel with a fake Linq and a fake model. This is the PR check.
//
// It runs the Worker with `wrangler dev --local`. `LINQ_API` points at a fake Linq, and the `AI`
// binding is a fake model Worker that returns scripted tool calls. Every run is the same.
// Each case checks only what users can see: what Satchel sends to Linq, what it sends to the
// model, and when a reminder fires. live.mjs runs the same Worker with the real model.
//
// Run: node test/e2e/run.mjs

import { createServer } from "node:http";
import { DatabaseSync } from "node:sqlite";
import {
  describe, eventOf, file, linq, listen, newUser, readJson, sleep, startWorker, stop, summarize,
  userDatabases, webhook,
} from "./harness.mjs";

const REPLY_TIMEOUT_MS = 20_000;
const QUIET_MS = 300;

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

/** The messages after the newest user message: this turn's tool calls and results. */
const thisTurn = (messages) => messages.slice(messages.findLastIndex((message) => message.role === "user") + 1);

const model = createServer(async (req, res) => {
  const { messages } = await readJson(req);
  const text = messages.findLast((message) => message.role === "user").content;
  const step = thisTurn(messages).filter((message) => message.role === "assistant").length;
  modelRequests.push({ text, messages });

  let reply = script.get(text)?.[step];
  if (typeof reply === "function") reply = await reply();
  // A due reminder comes back as a nudge with its links and files.
  if (text.startsWith("Reminder due: ")) reply = say(`nudge: ${text.slice(14).replaceAll("\n", " ")}`);
  if (!reply) return res.writeHead(500).end(`no scripted reply for ${JSON.stringify(text)} step ${step}`);
  res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ choices: [{ message: reply }] }));
});

/** The latest request Satchel sent the model while answering `text`. */
const lastModelRequest = (text) => modelRequests.findLast((request) => request.text === text);

/** The tool results Satchel sent the model for its latest answer to `text`. */
const toolResults = (text) => thisTurn(lastModelRequest(text).messages).filter((message) => message.role === "tool").map((message) => message.content);

const user = newUser();

/** Linq requests already checked by a case. */
let checked = 0;

/** Waits up to `timeout` for at least `min` new Linq requests, then until they stop. Returns them. */
async function nextRequests(timeout, min = 1) {
  const deadline = Date.now() + timeout;
  while (linq.requests.length - checked < min && Date.now() < deadline) await sleep(50);
  if (linq.requests.length === checked) return [];
  let count;
  do {
    count = linq.requests.length;
    await sleep(QUIET_MS);
  } while (linq.requests.length !== count);
  const requests = linq.requests.slice(checked);
  checked = linq.requests.length;
  return requests;
}

/** Sends `message` and returns Satchel's reply to it. */
async function turn(message) {
  const { eventId, response } = await webhook(user, message);
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
    const { response } = await webhook(user, "hello", { signature: "v1,bad" });
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
      const { eventId, response } = await webhook(user, text);
      if (!response.ok) return `webhook returned ${response.status}`;
      events.push(eventId);
    }
    const requests = await nextRequests(REPLY_TIMEOUT_MS, texts.length);
    const replies = requests.map((request) => `${eventOf(request)} ${request.body?.message?.parts[0].value}`);
    const expected = events.map((event, i) => `${event} reply ${i + 1}`);
    if (replies.join() !== expected.join()) return `replies were ${JSON.stringify(replies)}`;
    for (const [i, text] of texts.entries()) {
      const seen = lastModelRequest(text).messages.map((message) => message.content);
      if (texts.slice(i + 1).some((later) => seen.includes(later))) return `the model saw a later text while answering "${text}"`;
      if (i > 0 && !seen.includes(`reply ${i}`)) return `the model did not see "reply ${i}" while answering "${text}"`;
    }
  }],

  // Linq sends a webhook again when it gets no answer. The copy gets no second reply.
  ["duplicate-webhook", async () => {
    const message = "same text twice";
    script.set(message, [say("one reply")]);
    const { eventId } = await webhook(user, message);
    await webhook(user, message, { eventId });
    const reply = summarize(await nextRequests(REPLY_TIMEOUT_MS), eventId);
    if (reply.text !== "one reply" || reply.late.length > 0) return `sent ${describe(reply)}`;
  }],

  // A send that Linq refuses is sent again later, with the stored reply. The model does not run again.
  ["linq-refuses-send", async () => {
    const message = "linq is down";
    script.set(message, [say("stored reply")]);
    linq.refuseNextSend = true;
    const reply = await turn(message);
    if (linq.refused !== 1) return `Linq refused ${linq.refused} sends, not 1`;
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
    const { response } = await webhook(user, "hello everyone", { group: true });
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
    const message = "remind me to call mom";
    const remind = () => call(["remind", { note: "call mom", at: localTime(Date.now() + 4_000, ZONE), time_zone: ZONE }]);
    script.set(message, [remind, remind, say("set")]);
    const reply = await turn(message);
    if (reply.text !== "set") return `sent ${describe(reply)}`;
    const nudged = await nudge(12_000);
    if (!nudged?.text.includes("call mom")) return `nudge was ${describe(nudged)}`;
    const due = modelRequests.findLast((request) => request.text.startsWith("Reminder due: call mom"));
    if (!due.messages[0].content.includes(ZONE)) return "the model was not told the saved time zone";
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
  const databases = userDatabases();
  if (databases.length !== 1) return [`expected 1 User database, found ${databases.length}`];
  const db = new DatabaseSync(databases[0], { readOnly: true });
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
  await startWorker(["--local"], { fakeModel: `http://127.0.0.1:${await listen(model)}/` });
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
  model.close();
  stop();
}
process.exit(failures ? 1 : 0);
