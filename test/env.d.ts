import type { WorkerBindings } from "../src/worker/app";

declare global {
  namespace Cloudflare {
    interface Env extends WorkerBindings {}
  }
}
