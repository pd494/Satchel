import { HttpApp, HttpRouter, HttpServerResponse } from "@effect/platform";
import { ConfigProvider, Effect } from "effect";
import { recvMessage } from "./connection";
import { Hmac } from "./hmac";
import type { WorkerBindings } from "./types/bindings";
import { handlePhotonWebhook } from "./webhook";

/** Shared Cloudflare Worker routes. */
export const router = (bindings: WorkerBindings) =>
  HttpRouter.empty.pipe(
    HttpRouter.post(
      "/webhooks/photon",
      handlePhotonWebhook((message) => recvMessage(message, bindings.ACCOUNTS)),
    ),
    HttpRouter.get("/", HttpServerResponse.text("satchel.ok")),
  );

const configProviderFromBindings = (bindings: WorkerBindings) => {
  const values = new Map<string, string>();

  if (bindings.WEBHOOK_SECRET !== undefined)
    values.set("WEBHOOK_SECRET", bindings.WEBHOOK_SECRET);

  if (bindings.ACCOUNT_ID_SECRET !== undefined)
    values.set("ACCOUNT_ID_SECRET", bindings.ACCOUNT_ID_SECRET);

  return ConfigProvider.fromMap(values);
};

/** Cloudflare's Web-standard entry point; Effect runs inside handleRequest. */
export default {
  fetch(request: Request, bindings: WorkerBindings): Promise<Response> {
    const app = router(bindings).pipe(
      Effect.provide(Hmac.Default),
      Effect.withConfigProvider(configProviderFromBindings(bindings)),
    );

    return HttpApp.toWebHandler(app)(request);
  },
};
