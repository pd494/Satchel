import type { WorkerBindings } from "../src/types/bindings";

declare global {
  namespace Cloudflare {
    interface Env extends WorkerBindings {}
  }
}
