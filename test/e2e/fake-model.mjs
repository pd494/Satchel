// A stand-in for the Workers AI binding in the PR check. harness.mjs binds this Worker as `AI`.
// Its `run` takes the same arguments as Workers AI and forwards the input to run.mjs, which holds
// the scripted replies.

import { WorkerEntrypoint } from "cloudflare:workers";

export default class FakeModel extends WorkerEntrypoint {
  fetch() {
    return new Response("fake model");
  }

  async run(model, input) {
    const response = await fetch(this.env.SCRIPT_URL, { method: "POST", body: JSON.stringify(input) });
    if (!response.ok) throw new Error(`fake model: ${await response.text()}`);
    return response.json();
  }
}
