// Benchmark of chat search on a fixed test chat. Run it before and after any change to chat
// search, and keep the change only if the numbers say so.
//
// chat-corpus.json holds 117 messages from a live test chat: long texts with one detail in the
// middle, short questions asked more than once, and Satchel's replies. Each question has the user's
// words, the short query Gemma would send, and the words only its answer contains. A question with
// no answer checks what search returns when nothing fits. Each run tries both query styles.
//
// The steps copy `Harness::search_chat` and `Db::chat_matches`. The constants and the schema are
// read from the Rust source. A change there then shows up here. Only the reranker calls Workers AI,
// about 2 neurons a question.
//
// Run: node test/bench/chat-search.mjs
// It needs `CLOUDFLARE_API_TOKEN` or a wrangler login.

import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";

const source = (path) => readFileSync(new URL(`../../${path}`, import.meta.url), "utf8");
const harness = source("src/harness.rs");
const constant = (name) => Number(harness.match(new RegExp(`const ${name}: \\w+ = ([\\d_.]+);`))[1].replaceAll("_", ""));
const CHAT_MATCHES = constant("CHAT_MATCHES");
const CHAT_CANDIDATES = constant("CHAT_CANDIDATES");
const CHAT_HITS = constant("CHAT_HITS");
const PASSAGE = constant("PASSAGE");
const PASSAGE_STEP = constant("PASSAGE_STEP");
const SNIPPET = constant("SNIPPET");
const LOOKALIKE = constant("LOOKALIKE");
const RERANK_MODEL = source("src/agent.rs").match(/const RERANK_MODEL: &str = "([^"]+)"/)[1];
const SCHEMA = source("src/db.rs").match(/pub const SCHEMA: &str = r#"([\s\S]*?)"#;/)[1];
const ACCOUNT = source("wrangler.jsonc").match(/"account_id":\s*"(\w+)"/)[1];
const TOKEN = process.env.CLOUDFLARE_API_TOKEN ?? JSON.parse(execSync("npx -y wrangler@4 auth token --json").toString()).token;

const corpus = JSON.parse(readFileSync(new URL("chat-corpus.json", import.meta.url), "utf8"));

// The chat, in Satchel's schema. A reply needs the user text it answers. A second reply to one
// text has none and is left out.
const db = new DatabaseSync(":memory:");
db.exec(SCHEMA);
let lastUser = null;
let answered = false;
for (const [n, { role, content }] of corpus.messages.entries()) {
  if (role === "user") {
    lastUser = `e${n}`;
    answered = false;
    db.prepare("INSERT INTO messages (role, event_id, content, created_at) VALUES ('user', ?, ?, ?)").run(lastUser, content, n);
  } else if (lastUser && !answered) {
    answered = true;
    db.prepare("INSERT INTO messages (role, reply_to, content, created_at) VALUES ('assistant', ?, ?, ?)").run(lastUser, content, n);
  }
}

const ftsQuery = (query) => query.split(/[^0-9A-Za-z]+/).filter(Boolean).map((word) => `"${word}"*`).join(" OR ");

// Db::chat_matches with a query: both sides of the chat, best word match first.
const chatMatches = (query, limit) =>
  db.prepare(`SELECT chat.id, chat.turn, chat.content FROM chat JOIN messages_fts AS f ON f.rowid = chat.id
              WHERE messages_fts MATCH ? ORDER BY f.rank LIMIT ${limit}`).all(ftsQuery(query));

async function rerank(query, passages) {
  const response = await fetch(`https://api.cloudflare.com/client/v4/accounts/${ACCOUNT}/ai/run/${RERANK_MODEL}`, {
    method: "POST",
    headers: { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" },
    body: JSON.stringify({ query, contexts: passages.map((text) => ({ text })) }),
  });
  const body = await response.json();
  if (!body.success) throw new Error(JSON.stringify(body.errors));
  const scores = new Array(passages.length).fill(0);
  for (const { id, score } of body.result.response) scores[id] = score;
  return scores;
}

// Harness::search_chat with a query.
async function searchChat(query) {
  const candidates = [];
  const seen = new Set();
  for (const row of chatMatches(query, CHAT_MATCHES)) {
    const text = row.content.split(/\s+/).filter(Boolean).join(" ").toLowerCase();
    if (seen.has(text)) continue;
    seen.add(text);
    candidates.push(row);
    if (candidates.length === CHAT_CANDIDATES) break;
  }
  if (candidates.length === 0) return [];

  const passages = [];
  const owners = [];
  for (const [owner, row] of candidates.entries()) {
    const chars = [...row.content];
    for (let start = 0; ; start += PASSAGE_STEP) {
      const end = Math.min(start + PASSAGE, chars.length);
      passages.push(chars.slice(start, end).join(""));
      owners.push(owner);
      if (end === chars.length) break;
    }
  }
  const scores = await rerank(query, passages);
  const best = new Map();
  for (const [passage, owner] of owners.entries()) {
    if (!best.has(owner) || scores[passage] > best.get(owner).score) best.set(owner, { score: scores[passage], passage });
  }
  const ranked = [...best.values()].sort((a, b) => b.score - a.score);

  // Lookalikes: skip a passage that shares at least LOOKALIKE of its words with one already shown.
  const words = (text) => new Set(text.split(/[^\p{L}\p{N}]+/u).filter((w) => [...w].length > 2).map((w) => w.toLowerCase()));
  const shownWords = [];
  const shownTurns = new Set();
  const lines = [];
  for (const { passage } of ranked) {
    const passageWords = words(passages[passage]);
    let lookalike = false;
    for (const other of shownWords) {
      let shared = 0;
      for (const word of passageWords) {
        if (other.has(word)) shared += 1;
      }
      const all = passageWords.size + other.size - shared;
      if (all > 0 && shared / all >= LOOKALIKE) {
        lookalike = true;
        break;
      }
    }
    if (lookalike) continue;
    shownWords.push(passageWords);

    // A match shows its whole turn: the user's text and Satchel's reply.
    const hit = candidates[owners[passage]];
    if (shownTurns.has(hit.turn)) continue;
    shownTurns.add(hit.turn);
    for (const row of db.prepare("SELECT id, content FROM chat WHERE turn = ? ORDER BY id").all(hit.turn)) {
      lines.push(row.id === hit.id ? passages[passage] : row.content.slice(0, SNIPPET));
    }
    if (shownTurns.size === CHAT_HITS) break;
  }
  return lines;
}

// The design before reranking, for comparison: the top word matches of all messages, cut to
// SNIPPET characters.
const plainSearch = (query) =>
  db.prepare(`SELECT chat.content FROM chat JOIN messages_fts AS f ON f.rowid = chat.id
              WHERE messages_fts MATCH ? ORDER BY f.rank LIMIT ${CHAT_HITS}`).all(ftsQuery(query)).map((row) => row.content.slice(0, SNIPPET));

const answerable = corpus.questions.filter((q) => q.answer !== null).length;
for (const style of ["question", "query"]) {
  console.log(`queries ${style === "question" ? "as the user texts them" : "as Gemma writes them"}:`);
  for (const [name, search] of [["plain word search", plainSearch], ["current chat search", searchChat]]) {
    let visible = 0;
    let junk = 0;
    let chars = 0;
    const misses = [];
    for (const item of corpus.questions) {
      const shown = (await search(item[style])).join("\n");
      chars += shown.length;
      if (item.answer === null) junk += shown ? 1 : 0;
      else if (shown.toLowerCase().includes(item.answer.toLowerCase())) visible += 1;
      else misses.push(item.question);
    }
    console.log(`  ${name}: answer visible ${visible}/${answerable}, results on no-answer questions ${junk}/${corpus.questions.length - answerable}, average ${Math.round(chars / corpus.questions.length)} characters`);
    if (misses.length) console.log(`    missed: ${misses.join(" | ")}`);
  }
}
