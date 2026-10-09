// Scripted iMessage demo rendered inside the iPhone bezel.
(() => {
  const ui = document.getElementById("ui");
  const screen = ui.parentElement;
  const inner = document.getElementById("threadInner");
  const cmpText = document.getElementById("cmpText");
  const kbd = document.getElementById("kbd");
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

  // The status bar shows the visitor's real date and time.
  const now = new Date();
  const clock = now.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  document.querySelector(".sb-time").textContent = clock.replace(/\s?[AP]M$/, "");

  // Keep the 402pt-wide UI scaled to the measured screen cutout.
  new ResizeObserver(([entry]) => {
    ui.style.setProperty("--s", entry.contentRect.width / 402);
  }).observe(screen);

  // Pin the thread to its newest message as messages arrive or the keyboard opens.
  const thread = inner.parentElement;
  const pin = new ResizeObserver(() => { thread.scrollTop = thread.scrollHeight; });
  pin.observe(thread);
  pin.observe(inner);

  const art = window.satchelArt;

  const link = (kind, title, domain, duration) => ({ kind: "link", art: kind, title, domain, duration });

  // ---------- Script ----------
  const script = [
    ["stamp", `<b>Today</b> ${clock}`],
    ["type", "my back is killing me"],
    ["kbdown"],
    ["reply", "Here are some stretches you saved for this:"],
    ["recv", link("stretch", "10 Min Hip Flexor Routine for Lower Back Pain", "youtube.com", "10:24")],
    ["wait", 1400],
    ["type", "ok unrelated but i found this apartment"],
    ["paste", "maplecourtapts.com/units/2b", link("apartment", "2BR with Balcony | Maple Court", "maplecourtapts.com")],
    ["kbdown"],
    ["reply", "You mentioned a pool is non-negotiable. Does this one have one?"],
    ["type", "no unfortunately not"],
    ["kbdown"],
    ["reply", "Tossed it in your satchel. Noted: no pool."],
  ];

  // ---------- Keyboard ----------
  const rows = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
  kbd.innerHTML =
    `<div class="kb-row">${[...rows[0]].map((k) => `<span class="key" data-k="${k}">${k}</span>`).join("")}</div>` +
    `<div class="kb-row r2">${[...rows[1]].map((k) => `<span class="key" data-k="${k}">${k}</span>`).join("")}</div>` +
    `<div class="kb-row r3"><span class="key mod wide"><svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"><path d="M10 3 3 10h4v6h6v-6h4Z"/></svg></span>${[...rows[2]].map((k) => `<span class="key" data-k="${k}">${k}</span>`).join("")}<span class="key mod wide"><svg width="24" height="18" viewBox="0 0 24 18" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2h13a1.5 1.5 0 0 1 1.5 1.5v11A1.5 1.5 0 0 1 21 16H8l-6.5-7Z"/><path d="m11 6 6 6m0-6-6 6"/></svg></span></div>` +
    `<div class="kb-row r4"><span class="key mod" data-k="123">123</span><span class="key space" data-k=" ">space</span><span class="key mod" data-k="return">return</span></div>` +
    `<div class="kb-extra"><svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.6 3.7 5.6 3.7 9s-1.2 6.4-3.7 9c-2.5-2.6-3.7-5.6-3.7-9S9.5 5.6 12 3Z"/></svg><svg width="20" height="26" viewBox="0 0 14 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><rect x="3.5" y="1" width="7" height="11.5" rx="3.5"/><path d="M1 9.5a6 6 0 0 0 12 0M7 15.5V19"/></svg></div>`;

  const press = (ch) => {
    const k = /[a-z ]/.test(ch) ? ch : /[A-Z]/.test(ch) ? ch.toLowerCase() : "123";
    const el = kbd.querySelector(`[data-k="${CSS.escape(k)}"]`);
    if (!el) return;
    el.classList.add("down");
    setTimeout(() => el.classList.remove("down"), 90);
  };

  // ---------- Thread rendering ----------
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

  let lastSide = null;
  let receipt = null;
  const lastMsg = () => [...inner.querySelectorAll(".msg")].pop();

  const bubble = (side, msg) => {
    const row = document.createElement("div");
    row.className = `msg ${side} in`;
    if (side !== lastSide) row.classList.add("first");
    else lastMsg()?.querySelector(".b")?.classList.remove("tail");
    lastSide = side;

    const b = document.createElement("div");
    if (msg.kind === "link") {
      b.className = "b link tail";
      const dur = msg.duration ? `<span class="card-dur">${msg.duration}</span>` : "";
      b.innerHTML = `<div class="card"><div class="card-art">${art[msg.art]}${dur}</div><div class="card-meta"><div class="card-title">${esc(msg.title)}</div><div class="card-domain">${esc(msg.domain)}</div></div></div>`;
    } else if (msg.kind === "typing") {
      b.className = "b typing tail";
      b.innerHTML = "<span></span><span></span><span></span>";
    } else {
      b.className = "b tail";
      b.textContent = msg.text;
    }
    row.appendChild(b);
    inner.append(row);
    return row;
  };

  const stamp = (html) => {
    const el = document.createElement("div");
    el.className = "stamp";
    el.innerHTML = html;
    lastSide = null;
    inner.append(el);
  };

  const setReceipt = (html) => {
    receipt?.remove();
    const sent = [...inner.querySelectorAll(".msg.sent")].pop();
    if (!sent) return;
    receipt = document.createElement("div");
    receipt.className = "rcpt";
    receipt.innerHTML = html;
    sent.after(receipt);
  };

  const reset = () => {
    inner.replaceChildren();
    inner.classList.remove("fading");
    cmpText.textContent = "";
    ui.classList.remove("kb-on", "has-text");
    lastSide = null;
    receipt = null;
  };

  // ---------- Runner ----------
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, reduced ? 0 : ms));

  const openKeyboard = async () => {
    if (ui.classList.contains("kb-on")) return;
    ui.classList.add("kb-on");
    await sleep(380);
  };

  const typeInto = async (text) => {
    await openKeyboard();
    ui.classList.add("has-text");
    if (reduced) {
      cmpText.textContent = text;
      return;
    }
    for (const ch of text) {
      cmpText.textContent += ch;
      press(ch);
      await sleep(ch === " " ? 55 + Math.random() * 40 : 28 + Math.random() * 34);
    }
  };

  // Sends whatever is in the composer as a bubble, then shows "Delivered".
  const send = async (text, sendAs) => {
    await sleep(250);
    press("return");
    cmpText.textContent = "";
    ui.classList.remove("has-text");
    receipt?.remove();
    bubble("sent", sendAs ?? { kind: "text", text });
    await sleep(300);
    setReceipt("Delivered");
    await sleep(350);
  };

  const steps = {
    async stamp(html) {
      stamp(html);
      await sleep(300);
    },
    async type(text, sendAs) {
      await typeInto(text);
      await send(text, sendAs);
    },
    // A pasted link lands in the field all at once, then sends.
    async paste(text, sendAs) {
      await openKeyboard();
      await sleep(350);
      cmpText.textContent = text;
      ui.classList.add("has-text");
      await sleep(500);
      await send(text, sendAs);
    },
    async kbdown() {
      ui.classList.remove("kb-on");
      await sleep(300);
    },
    async reply(text) {
      setReceipt("<b>Read</b> just now");
      await sleep(300);
      const typing = bubble("recv", { kind: "typing" });
      await sleep(700 + Math.min(text.length * 10, 700));
      typing.remove();
      lastSide = lastMsg()?.classList.contains("recv") ? "recv" : "sent";
      bubble("recv", { kind: "text", text });
      await sleep(600);
    },
    async recv(msg) {
      bubble("recv", msg);
      await sleep(600);
    },
    async wait(ms) {
      await sleep(ms);
    },
  };

  const mobile = matchMedia("(max-width: 899px)").matches;
  let visible = !mobile;
  let playing = false;

  // On mobile, wait until half the phone is visible before the first turn.
  // Loops while the phone stays on screen; reduced motion plays once.
  const play = async () => {
    playing = true;
    do {
      reset();
      await sleep(400);
      for (const [name, ...args] of script) await steps[name](...args);
      if (reduced) break;
      await sleep(4000);
      inner.classList.add("fading");
      await sleep(450);
      reset();
    } while (visible);
    playing = false;
  };

  new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting && (!mobile || entry.intersectionRatio >= 0.5);
    if (visible && !playing && !inner.childElementCount) play();
  }, { threshold: [0, 0.5] }).observe(document.getElementById("phone"));
  if (!mobile) play();
})();
