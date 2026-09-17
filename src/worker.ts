import { HttpApp, HttpRouter, HttpServerResponse } from "@effect/platform";
import { ConfigProvider, Effect } from "effect";
import { handlePhotonWebhook } from "./webhook";

export interface WorkerBindings {
  readonly WEBHOOK_SECRET?: string;
}

/** Shared Cloudflare Worker routes. */
export const router = HttpRouter.empty.pipe(
  HttpRouter.post("/webhooks/photon", handlePhotonWebhook()),
  HttpRouter.get("/", HttpServerResponse.text("satchel.ok")),
);

const configProviderFromBindings = (bindings: WorkerBindings) => {
  const values = new Map<string, string>();

  if (bindings.WEBHOOK_SECRET !== undefined)
    values.set("WEBHOOK_SECRET", bindings.WEBHOOK_SECRET);

  return ConfigProvider.fromMap(values);
};

/** Cloudflare's Web-standard entry point; Effect runs inside handleRequest. */
export default {
  fetch(request: Request, bindings: WorkerBindings = {}): Promise<Response> {
    const app = router.pipe(
      Effect.withConfigProvider(configProviderFromBindings(bindings)),
    );

    return HttpApp.toWebHandler(app)(request);
  },
};
