import { HttpApp, HttpRouter, HttpServerResponse } from "@effect/platform";
import { handlePhotonWebhook } from "./photon-webhook";

/** Shared Cloudflare Worker routes. */
export const router = HttpRouter.empty.pipe(
  HttpRouter.post("/webhooks/photon", handlePhotonWebhook()),
  HttpRouter.get("/", HttpServerResponse.text("satchel.ok")),
);

const handleRequest = HttpApp.toWebHandler(router);

/** Cloudflare's Web-standard entry point; Effect runs inside handleRequest. */
export default {
  fetch(request: Request): Promise<Response> {
    return handleRequest(request);
  },
};
