# Implementing link previews

Step-by-step tutorial. No code here — what to build, where, docs to read, and how to know it works. Testing (what we already ran + what you must re-check) lives in §3–§4.

Tested 2026-11-04. Fetch UA: `Satchel/0.1`. Blocked rows were re-run from a real Cloudflare Worker (`wrangler dev --remote`); those results win.

## Status (updated 2026-11-05)

**Done** (compiles; not yet run on a real link): steps 1, 5, 6, 7.
**Left:** steps 2, 3, 4, 8, 9, 10, 11, then the live tests in §4.

**What changed from this guide while building:**

- `Media` is a **trait**, not an enum. Each type (`Link` now; voice memo, note later) owns its own fields and implements it in its own file (`src/media.rs`, `src/media/link.rs`).
- Names: `Media::as_str` is `get_type`. `Link::fetch` is `Link::build_link`.
- `saved` has a `data` JSON column for fields only one type has (a link's `image_url`, `site_name`, `image_alt`, `author`). It replaces the `image_url` column. Wipe `.wrangler/state` once.
- oEmbed uses **discovery**, not a hardcoded host list. Checked 2026-11-05 with a plain request: X and Spotify advertise an endpoint; YouTube, Vimeo, Reddit, TikTok did not.
- The page is read as a stream and stops at `</head>` or 500 KB.
- Links arrive from Photon as a plain `text` message that holds only the URL (checked on 25 real messages). A photo with a caption arrives as a `group` of parts.
- No unit tests. The project has none, so §4 is checked by live runs.



## 0. Before you start

**Know these** (read once, ~10 min each):

- [ ] Open Graph: `og:title`, `og:description`, `og:image` — [ogp.me](https://ogp.me)
- [ ] oEmbed JSON for a URL — [oembed.com](https://oembed.com) §2.2
- [ ] Worker fetch in Rust: `[Fetch](https://docs.rs/worker/latest/worker/enum.Fetch.html)`, `[Request::new_with_init](https://docs.rs/worker/latest/worker/struct.Request.html)`, `[RequestInit](https://docs.rs/worker/latest/worker/struct.RequestInit.html)`, `[Headers](https://docs.rs/worker/latest/worker/struct.Headers.html)`
- [ ] Stream HTML parse: `[lol_html](https://docs.rs/lol_html)` (`rewrite_str`, `RewriteStrSettings`, `element!`, `text!`); entities: [html-escape](https://docs.rs/html-escape)
- [ ] URLs: `[worker::Url](https://docs.rs/url)` (`parse`, `host_str`, `join`)
- [ ] DO alarms: [Cloudflare docs](https://developers.cloudflare.com/durable-objects/api/alarms/) — work after the reply, with retries

**Ready:**

- [ ] `cargo check` passes on `main`
- [ ] `wrangler whoami` shows you're logged in
- [x] `saved` has `title`, `description`, `data` (JSON), `attempts`, `media`. The schema changed: wipe `.wrangler/state` once.

**Rules that don't bend:**

- Only the **Harness** writes to the DB. The agent never does.
- Fetch with UA `Satchel/0.1`. Don't fake a browser (we tested it; it does worse).
- Out of scope for this card: vision captions, JSON-LD, embeddings, the X API.



## 1. Decision and shape

```
link
 ├─ og: tags (fallback twitter:, then <title>, meta description)
 └─ page advertises oEmbed? → oEmbed, merged in field by field
 got og:image? → vision caption (later card)
 title empty / generic / 403? → agent asks "what's this one?"
                                user's answer → description
```

```
User (DO) ── Harness ──┬── Db       (writes)
                       ├── Agent    (reads, talks)
                       └── Media    (Link::build_link: fetch + parse, no DB)
```

```
text in → Harness saves link row → reply "saved"
                     ↓ alarm
          Link::build_link(url) → title/desc/image
                     ↓
          Harness writes them (or marks "thin")
                     ↓ next turn
          Agent asks "what's this one?" if thin
```

No separate `Preview` trait. The `Media` trait covers every saved type; fetching stays inside `Link`.

## 2. Steps



### Step 1: `Media` and `Link` types

- [x] `src/media.rs`: `trait Media` (not an enum) with `get_type`, `content`, `title`, `description`, `data`. Each type implements it in its own file.
- [x] `src/media/link.rs`: `struct Link` owns its fields (`url`, `title`, `description`, `image_url`, `site_name`, `image_alt`, `author`) and `impl Media for Link`. `get_type` returns `"link"` to match `saved.media IN ('link')`.

- **Done when:** `cargo check` passes.
- **Think about:** why `Option`? Most sites miss at least one field (see §3).



### Step 2: pull the link out of the message

```
text ─▶ linkify (url_must_have_scheme(false)) ─▶ Vec<Url>   (no scheme → add https://)
             ├─ ≥1 link ─▶ saved row per link (Step 3) ─▶ Link::build_link later
             └─ 0 links ─▶ save the text as a Note (nothing lost)
```

- [ ] `find_links(text)` in `link.rs` with the `linkify` crate. Merciless: every match is a link. No model call.
- [ ] Keep the rest of the text (cut out each match with `.start()` / `.end()`). It becomes the description if the user wrote something.
- [ ] No match → save the text as a `Note`.

- **Done when:** "check this [https://a.com/x](https://a.com/x) lol" → `https://a.com/x` and "check this lol".
- **Edge cases:** two links in one message, no scheme (`t.co/...`), trailing `)` or `.`.
- **Tested 2026-11-07:** one URL shape per site in §3, plus message shapes: 63 of 69 pass.
- **Must fix:** iOS smart quotes (`“nytimes.com/x”`) and an emoji glued on (`pd494.ai🔥`) end up inside the match → wrong host. Trim them from the match ends.
- **Known misses:** `localhost:3000`, `pd494 dot ai`, a URL split by a newline → no match or cut → Note covers it. `README.md` → false link (`.md` is a TLD). Later: clef-flash `noul` per bare match to drop these.

**Why not a model (tested 2026-11-05)** (prompt: "reply with only a JSON array", temperature 0):


| Model                   | Long IG / Maps / Amazon / X URLs | `t.co/...` (no scheme) |
| ----------------------- | -------------------------------- | ---------------------- |
| `llama-3.2-3b-instruct` | exact                            | missed                 |
| `granite-4.0-h-micro`   | exact                            | missed                 |
| `llama-3.2-1b-instruct` | X URL looped until max tokens    | found                  |




### Step 3: save first, reply fast

- [ ] `Db` method inserts a `saved` row (`media = 'link'`, `content = url`), ignores duplicates via `UNIQUE`.
- [ ] Harness calls it, then replies "saved". Preview isn't fetched yet.

- **Done when:** a text with a link creates one row; the same event replayed creates no second row.



### Step 4: schedule the fetch with an alarm

- [ ] After saving, Harness sets a DO alarm (a few seconds out).
- [ ] Alarm handler asks `Db` for rows that still need a preview: `title IS NULL AND attempts < 3`.

- **Why alarms, not** `wait_until`**:** durable + retried. Worker dies mid-fetch → nothing lost.
- **Done when:** alarm fires and logs the URLs it would fetch.



### Step 5: fetch the page

- [x] `Link::build_link` GET with `User-Agent: Satchel/0.1`, following redirects.
- [x] Keep the **final URL**. Short links (`maps.app.goo.gl`, `amzn.to`, `t.co`) need it.
- [x] Non-2xx (403, 202, 419): empty preview, not an error → "thin".
- [x] Network error: `Err` so the alarm retries.

- **Done when:** fetching the Luma link from §3 returns 200 and some HTML.



### Step 6: oEmbed first for the sites that have it

- [x] Discovery, not a host list: read `<link type="application/json+oembed" href=...>` from the page head. Checked 2026-11-05: X and Spotify advertise it; YouTube, Vimeo, Reddit, TikTok did not (plain request), so they use `og:` only.
- [x] Call the advertised endpoint with `format=json` added. A failed call keeps the page data.
- [x] Map `title` / `author_name` (to `author`) / `provider_name` (to `site_name`) / `thumbnail_url`. No `title` (X): strip tags from `html`, then decode entities.
- [x] Merge with the `og:` result field by field. oEmbed wins.

- **Done when:** X → tweet text; Spotify → title + thumbnail.
- **Read:** `[serde_json::Value](https://docs.rs/serde_json)` is fine; no typed struct per provider.



### Step 7: parse og: tags for everything else

- [x] `lol_html` selectors: `meta[property^="og:"]`, `meta[name^="twitter:"]`, `meta[name="description"]`, `meta[name="author"]`, `title`.
- [x] Order: `og:` → `twitter:` → `<title>` (description: `og:` → `twitter:` → `meta description`).
- [x] Also read `og:site_name` and `og:image:alt` / `twitter:image:alt`.
- [x] Relative `og:image` → absolute with `Url::join` against the final URL.
- [x] Decode HTML entities (`&`, `"` showed up in real IG titles).
- [x] Stop after `</head>` or ~500 KB (read as a stream, so the page is never fully buffered).

- **Done when:** IG reel `DMyUy3xTg2B` → caption + image.



### Step 8: detect "thin" results

- [ ] Thin if title missing, or on this list (one `const`): "Instagram", "Google Maps", "Reddit", "TikTok - Make Your Day", "Just a moment...", "Access Denied", "Access to this page has been denied", "Log in", a bare domain.

- **Done when:** Yelp, Zillow, Reddit all come back thin.



### Step 9: write the result (Harness only)

- [ ] `Db` method takes row id + `&Link`, updates `title` / `description` / `image_url`, bumps `attempts`.
- [ ] Thin: still bump `attempts` and store whatever you got, so the row stops retrying.

- **Done when:** after the alarm the row has data; a second alarm does nothing.



### Step 10: the agent asks, the Harness saves

- [ ] When loading context, Harness tells the agent which recent saves are thin (id + URL).
- [ ] Tool like `describe(id, text)`. Agent *calls* it; **Harness** validates id and writes `description`.
- [ ] System prompt: if thin, ask once, briefly ("what's this one?"). Never twice.

- **Done when:** Yelp link → Satchel asks → you answer "tartine bakery" → row description is that.



### Step 11: retrieval works

- [ ] Agent search reads `title`, `description`, `created_at`.
- [ ] System prompt: expand synonyms before searching ("hips" → hips, hip, mobility, stretch).

- **Done when:** §4 retrieval checks pass.



## 3. What we tested (55 URLs)

✅ usable · ⚠️ generic (site name / block page) · ❌ missing


| Site                                                        | HTTP      | Title | Desc | Image | Best source     | Notes                                                                                          |
| ----------------------------------------------------------- | --------- | ----- | ---- | ----- | --------------- | ---------------------------------------------------------------------------------------------- |
| Substack                                                    | 200       | ✅     | ✅    | ✅     | og:             |                                                                                                |
| Verge, TechCrunch, NYT                                      | 200       | ✅     | ✅    | ✅     | og:             | front pages only                                                                               |
| BBC                                                         | 200       | ✅     | ✅    | ❌     | og:             | front page; article link was dead                                                              |
| Personal blog (paulgraham.com)                              | 200       | ✅     | ❌    | ❌     | `<title>`       |                                                                                                |
| AP, Medium                                                  | 200       | ✅     | ✅    | ✅     | og:             | 403 from a server IP, works from a Worker                                                      |
| X (text + video tweets)                                     | 200       | ⚠️    | ✅    | ✅     | oEmbed          | og title is "Name (@handle) on X"; oEmbed has no media                                         |
| Threads, Bluesky, LinkedIn                                  | 200       | ✅     | ✅    | ✅     | og:             | LinkedIn oEmbed 404                                                                            |
| Reddit                                                      | 200       | ⚠️    | ❌    | ❌     | oEmbed          | og gives generic title                                                                         |
| TikTok                                                      | 200       | ⚠️    | ❌    | ❌     | oEmbed / ask    | oEmbed = player only                                                                           |
| Instagram reel                                              | 200       | ✅     | ✅    | ✅     | og:             | author, short caption, likes, date, thumbnail; Meta oEmbed returns no thumbnail since Nov 2025 |
| YouTube                                                     | 200       | ✅     | ✅    | ✅     | oEmbed          |                                                                                                |
| Spotify, Apple Music, Apple Podcasts                        | 200       | ✅     | ✅    | ✅     | og:             |                                                                                                |
| Shopify (Allbirds)                                          | 200       | ✅     | ✅    | ✅     | og:             |                                                                                                |
| Amazon /dp/                                                 | 200       | ✅     | ❌    | ❌     | og: + ask       | real title, no image                                                                           |
| Etsy, Yelp, Zillow, StreetEasy, Apartments.com              | 403       | ⚠️    | ❌    | ❌     | ask             | blocked even from a Worker; Microlink also failed                                              |
| Airbnb listing                                              | 200       | ✅     | ✅    | ✅     | og:             |                                                                                                |
| Google Maps (place + maps.app.goo.gl)                       | 200       | ⚠️    | ✅    | ✅     | og: + final URL | title is "Google Maps"; place name + coords in redirected URL                                  |
| OpenTable                                                   | 200       | ✅     | ✅    | ✅     | og:             |                                                                                                |
| Recipe blogs                                                | 200       | ✅     | ✅    | ✅     | og:             |                                                                                                |
| NYT Cooking                                                 | 200       | ✅     | ✅    | ✅     | og:             | empty from a server IP, works from a Worker                                                    |
| Luma, Partiful, Eventbrite                                  | 200       | ✅     | ✅    | ✅     | og:             | event date only in JSON-LD (later)                                                             |
| GitHub, Notion, public Google Doc, App Store                | 200       | ✅     | ✅    | ✅     | og:             | private Doc → 404 → ask                                                                        |
| Figma                                                       | 403       | ⚠️    | ❌    | ❌     | ask             | blocked even from a Worker                                                                     |
| Pinterest, Vimeo, SoundCloud, Goodreads, Letterboxd, Twitch | 200       | ✅     | ✅    | ✅     | og:             |                                                                                                |
| Wikipedia                                                   | 200       | ✅     | ❌    | ✅     | og:             |                                                                                                |
| Hacker News                                                 | 200 / 419 | ✅     | ❌    | ❌     | `<title>`       | 419 (rate limit) from a Worker; retry                                                          |
| IMDb                                                        | 202       | ❌     | ❌    | ❌     | ask             | bot challenge, same from a Worker                                                              |
| Facebook post                                               | 200       | ✅     | ✅    | ❌     | og:             |                                                                                                |


**Coverage:** 65% title + image, ~82% usable title. The rest → ask.

**Not tested yet:** `amzn.to` short links, single news articles (we hit front pages), Instagram oEmbed with a real token, Instagram carousels (og: likely first slide only).

**Column mapping once saved:**


| Column        | From                                         |
| ------------- | -------------------------------------------- |
| `content`     | final URL (after redirects)                  |
| `title`       | oEmbed title / og:title / `<title>`          |
| `description` | og:description, tweet text, or user's answer |
| `image_url`   | og:image (thumbnail URLs can expire)         |


**How it comes back:**

```
"that hip stretching reel"
 → agent searches saved (title, description, created_at)
 → hit → reply with title + URL
 → miss → ask a narrowing question (when? which app?)
```

Works today (once built): anything whose title/description has the words.
Fails today: media-only posts (text only on the thumbnail), vague visual asks ("the purple one"), synonyms ("hips" vs "pigeon pose") until vision / embeddings.

**Edge cases:**


| Case                                               | Handling                                                  |
| -------------------------------------------------- | --------------------------------------------------------- |
| Short links (`t.co`, `amzn.to`, `maps.app.goo.gl`) | follow redirects, save final URL                          |
| Same link saved twice                              | `UNIQUE (message_id, content)`; agent lists both if asked |
| Post deleted later                                 | we still have stored title/description                    |
| Private / login-walled                             | generic title → ask                                       |
| Link + user text in one message                    | user text → description                                   |
| Fetch fails / times out                            | keep the row, `attempts += 1`, retry                      |




## 4. Test checklist (you run these)

**Unit tests:** skipped, the project has none. Check these by live runs instead:

- [ ] IG reel → title, description, image
- [ ] Substack → all three
- [ ] Hacker News → `<title>` only
- [ ] Yelp block page → thin
- [ ] Relative `og:image` → absolute URL
- [ ] Entities decoded (`&` → `&`)

**Live tests** (`wrangler dev --remote`). Text Satchel each and check the row:


| Link                   | Expect                    |
| ---------------------- | ------------------------- |
| YouTube video          | title, thumbnail          |
| IG reel `DMyUy3xTg2B`  | caption, image            |
| Luma event             | title, description, image |
| Amazon `/dp/`          | title only                |
| `maps.app.goo.gl` link | final URL saved           |
| Yelp                   | thin → Satchel asks       |
| Same link twice        | one row                   |


**Retrieval** (ask Satchel):

- [ ] "the morning mobility reel" → finds the IG reel
- [ ] "that bakery" → finds the Yelp link (via your answer)
- [ ] "what did I save yesterday" → lists by date



## 5. Done means

- [ ] Every row in the live table matches
- [ ] Reply "saved" arrives before the preview is fetched
- [ ] Killing the Worker mid-fetch → alarm retries, nothing lost
- [ ] Agent has no DB write path; `describe` goes through the Harness
- [ ] `cargo check` and `cargo clippy` are clean



## 6. Next cards (not this one)

1. Vision caption: any `image_url` → caption → append to `description`
2. JSON-LD: event dates, recipes, prices
3. Embeddings / semantic search

