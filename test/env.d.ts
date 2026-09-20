import type { WorkerBindings } from "../src/worker";

declare global {
  namespace Cloudflare {
    interface Env extends WorkerBindings {}
  }
}
