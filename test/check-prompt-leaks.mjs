// Fails when a test question appears word for word in a prompt. A prompt example that copies a
// test case makes that case pass because the model copies the example, not because the rule works.
// Texts shorter than 4 words, such as "hi" or "pst", are skipped.
//
// Run: node test/check-prompt-leaks.mjs (lint.yml runs it on every PR)

import { readdirSync, readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const normalize = (text) => text.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();

const tests = [];
for (const [, text] of read("test/applescript/satchel.applescript").matchAll(/\{"[\w-]+", "((?:[^"\\]|\\.)*)"/g)) {
  for (const part of text.split("||")) tests.push(part);
}
for (const { question } of JSON.parse(read("test/bench/chat-corpus.json")).questions) tests.push(question);

const prompts = readdirSync(new URL("../src/prompts", import.meta.url)).map((name) => [name, normalize(read(`src/prompts/${name}`))]);

let leaks = 0;
for (const text of tests) {
  const normalized = normalize(text);
  if (normalized.split(" ").length < 4) continue;
  for (const [name, prompt] of prompts) {
    if (prompt.includes(normalized)) {
      console.error(`src/prompts/${name} contains the test text "${text}"`);
      leaks += 1;
    }
  }
}
if (leaks) process.exit(1);
console.log(`ok: ${tests.length} test texts, none in src/prompts`);
