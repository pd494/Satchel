import { HttpApp, HttpRouter, HttpServerResponse } from "@effect/platform";

/** Shared Cloudflare Worker routes. Feature branches add routes here. */
export const router = HttpRouter.empty.pipe(
  HttpRouter.get("/", HttpServerResponse.text("satchel.ok")),
);

const handleRequest = HttpApp.toWebHandler(router);

/** Cloudflare's Web-standard entry point; Effect runs inside handleRequest. */
export default {
  fetch(request: Request): Promise<Response> {
    return handleRequest(request);
  },
};
