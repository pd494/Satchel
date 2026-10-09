const SITEVERIFY_URL = "https://challenges.cloudflare.com/turnstile/v0/siteverify";
const TEST_SITE_KEY = "1x00000000000000000000AA";
const TEST_SECRET = "1x0000000000000000000000000000000AA";
const isLocal = (hostname) => hostname === "localhost" || hostname === "127.0.0.1";

// Null when Turnstile is misconfigured for this hostname, so signups fail closed.
// Test keys and loopback hostnames are only accepted when serving loopback.
export function turnstileConfig(env, hostname) {
  const hostnames = (env.TURNSTILE_HOSTNAMES ?? "").split(",").map((h) => h.trim()).filter(Boolean);
  if (!env.TURNSTILE_SECRET || !env.TURNSTILE_SITE_KEY || !hostnames.includes(hostname)) return null;
  const testKey = /^[123]x0+$/.test(env.TURNSTILE_SITE_KEY.slice(0, -2));
  if (!isLocal(hostname) && (testKey || hostnames.some(isLocal))) return null;
  return {
    sitekey: env.TURNSTILE_SITE_KEY,
    action: env.TURNSTILE_ACTION || "signup",
    // Official dummy keys answer with hostname example.com and no action.
    localTest: hostnames.every(isLocal) && env.TURNSTILE_SITE_KEY === TEST_SITE_KEY && env.TURNSTILE_SECRET === TEST_SECRET,
  };
}

export async function verifyTurnstile(token, ip, env, hostname, fetcher = fetch) {
  const config = turnstileConfig(env, hostname);
  if (!config) return "unavailable";
  if (typeof token !== "string" || !token || token.length > 2048) return "invalid";
  try {
    const response = await fetcher(SITEVERIFY_URL, {
      method: "POST",
      body: new URLSearchParams({ secret: env.TURNSTILE_SECRET, response: token, remoteip: ip }),
      signal: AbortSignal.timeout(5000),
    });
    if (!response.ok) return "unavailable";
    const result = await response.json();
    if (result?.success !== true) return "invalid";
    const ok = config.localTest
      ? result.metadata?.result_with_testing_key === true
      : result.action === config.action && result.hostname === hostname;
    return ok ? "valid" : "invalid";
  } catch {
    return "unavailable";
  }
}
