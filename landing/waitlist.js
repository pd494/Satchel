import { normalizeEmail } from "./email.js";

const form = document.getElementById("waitlist");
const input = form.elements.email;
const button = form.querySelector("button");
const status = document.getElementById("waitlist-status");
const toast = document.getElementById("signup-toast");

const INVALID_EMAIL = "Please enter a valid email address.";
// Worker error codes → [where to show it, message].
const ERRORS = {
  invalid_email: ["status", INVALID_EMAIL],
  already_registered: ["toast", "You’re already on the waitlist."],
  rate_limited: ["toast", "Too many attempts. Please try again in a minute."],
  verification_failed: ["status", "Verification expired. Please try again."],
};

let widgetId;
let token = "";
let submitting = false;

const setToken = (value) => {
  token = value;
  button.disabled = !token || submitting;
};

let toastTimer;
const showToast = (message) => {
  clearTimeout(toastTimer);
  status.textContent = "";
  toast.textContent = message;
  toast.hidden = false;
  toastTimer = setTimeout(() => { toast.hidden = true; }, 6000);
};

// Turnstile's api.js is a deferred script ahead of this module, so it has
// already run. It retries failed challenges and refreshes expired tokens itself.
async function startVerification() {
  setToken("");
  try {
    const response = await fetch("/api/turnstile", { signal: AbortSignal.timeout(15000) });
    if (!response.ok || !window.turnstile) throw new Error("Verification unavailable");
    const { sitekey, action } = await response.json();
    widgetId = window.turnstile.render("#signup-turnstile", {
      sitekey,
      action,
      appearance: "interaction-only",
      callback: (value) => { setToken(value); status.textContent = ""; },
      "expired-callback": () => setToken(""),
      "error-callback": () => { setToken(""); status.textContent = "Verifying you’re human…"; },
    });
  } catch {
    status.textContent = "Verification couldn’t load. Please refresh and try again.";
  }
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (button.disabled) return;
  const email = normalizeEmail(input.value);
  if (!email) {
    status.textContent = INVALID_EMAIL;
    input.focus();
    return;
  }
  submitting = true;
  setToken(token);
  status.textContent = "Saving your spot…";
  try {
    const response = await fetch(form.action, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email, token }),
      signal: AbortSignal.timeout(15000),
    });
    if (response.ok) {
      showToast("You’re on the list. We’ll email you when Satchel launches.");
      form.reset();
      return;
    }
    const { error } = await response.json().catch(() => ({}));
    const [where, message] = ERRORS[error] ?? ["status", "Couldn’t save your email. Please try again."];
    if (where === "toast") showToast(message);
    else status.textContent = message;
  } catch {
    status.textContent = "Couldn’t save your email. Please try again.";
  } finally {
    // Tokens are single-use, so every attempt needs a fresh one.
    submitting = false;
    setToken("");
    window.turnstile?.reset(widgetId);
  }
});

startVerification();
