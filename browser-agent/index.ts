/**
 * FlowRad browser-automation sidecar (SPIKE).
 *
 * Drives a CDP-controlled Chrome through Stagehand v3 to run ONE browser task
 * per invocation. Stagehand v3 speaks the Chrome DevTools Protocol directly
 * (its "understudy" CDP client), so clicks land on real DOM nodes instead of
 * flattened UIA rectangles — which is exactly the failure mode that makes
 * browser tasks (playing a YouTube result, driving Grok's chat box) the
 * flakiest part of the live UIA path.
 *
 * Protocol (line-delimited JSON over stdio):
 *   stdin  <- one JSON line:  { "intent": "play the first result", "url"?: "...", "timeoutMs"?: 60000 }
 *   stdout -> one JSON line:  { "ok": true,  "detail": "...", "actions"?: [...] }
 *                        or:  { "ok": false, "detail": "...", "error": "..." }
 *
 * The Rust caller (src-tauri/src/act/browser.rs, feature `cdp-browser`) relies
 * on the RESULT LINE, not the exit code. We still exit non-zero on failure for
 * humans running it by hand.
 *
 * This is a spike: isolated, feature-flagged, and NOT wired into the conductor.
 * See ./README.md and docs/act-cdp-browser.md.
 */

import { Stagehand } from "@browserbasehq/stagehand";

// ---------------------------------------------------------------------------
// Configuration (all via env so the Rust host can inject per-launch values).
// ---------------------------------------------------------------------------

/** Gemini model that plans each act/observe turn. Provider prefix routes it to
 *  Google Generative AI inside Stagehand; the bare `gemini-3.6-flash` also works
 *  because Stagehand infers the provider from the name. */
const MODEL_NAME = process.env.FLOWRAD_BROWSER_MODEL ?? "google/gemini-3.6-flash";

/** Google Generative AI key. Required — Stagehand needs it to plan turns. */
const GEMINI_API_KEY = process.env.GEMINI_API_KEY ?? "";

/** DevTools remote-debugging port for the FlowRad Chrome. */
const CDP_PORT = Number.parseInt(process.env.FLOWRAD_CDP_PORT ?? "9222", 10);

/** Attach to an ALREADY-RUNNING Chrome at this CDP endpoint instead of launching
 *  a fresh one. e.g. "http://127.0.0.1:9222". Empty => Stagehand launches Chrome. */
const CDP_URL = process.env.FLOWRAD_CDP_URL ?? "";

/** Dedicated, PERSISTENT FlowRad profile. Kept separate from the user's daily
 *  Chrome so automation never fights their real session, tabs, or cookies, and
 *  so a login FlowRad performs once (Google, Grok) survives across runs. */
const USER_DATA_DIR =
  process.env.FLOWRAD_USER_DATA_DIR ??
  `${process.env.HOME ?? process.env.USERPROFILE ?? "."}/.flowrad/chrome-profile`;

/** Optional explicit Chrome/Chromium binary. Empty => Stagehand's default. */
const CHROME_PATH = process.env.FLOWRAD_CHROME_PATH ?? "";

/** Run Chrome without a visible window (useful in CI; off by default so the
 *  user can watch and take over). */
const HEADLESS = (process.env.FLOWRAD_HEADLESS ?? "false").toLowerCase() === "true";

/** Default per-task ceiling; a task's own `timeoutMs` overrides it. */
const DEFAULT_TIMEOUT_MS = Number.parseInt(process.env.FLOWRAD_TASK_TIMEOUT_MS ?? "60000", 10);

// ---------------------------------------------------------------------------
// Wire types.
// ---------------------------------------------------------------------------

/**
 * How to drive the task:
 *  - `"act"`   (default) — Stagehand plans the intent with the LLM and executes
 *                precise CDP clicks/typing on the real DOM node.
 *  - `"links"` — DETERMINISTIC, no LLM: enumerate the active tab's anchor links
 *                (text + resolved href) from the DOM, pick one (the Nth result or
 *                the best text match), and navigate the tab straight to its href.
 */
type TaskMode = "act" | "links";

interface Task {
  /** Natural-language goal for this browser turn. */
  intent: string;
  /** Optional URL to navigate to before acting (e.g. open YouTube first). */
  url?: string;
  /** Optional per-task timeout override, milliseconds. */
  timeoutMs?: number;
  /** Which driver to use. Defaults to `"act"`. */
  mode?: TaskMode;
  /**
   * `"links"`-mode selection hint. Either:
   *  - a 1-based position (`2` or `"2"`) → the Nth extracted link, or
   *  - text (`"lofi hip hop"`) → the best text match among the links.
   * When omitted, the task `intent` is used for a best-text match, falling back
   * to the first link.
   */
  select?: string | number;
}

/** One anchor extracted from the active tab, in DOM order. */
interface LinkCandidate {
  /** 1-based position among the extracted (http/https) anchors. */
  index: number;
  /** Collapsed visible link text. */
  text: string;
  /** Resolved absolute URL. */
  href: string;
}

interface TaskResult {
  ok: boolean;
  detail: string;
  /** Stagehand's structured actions, when the turn produced any (`act` mode). */
  actions?: unknown[];
  /** The href the tab was navigated to (`links` mode). */
  chosenHref?: string;
  /** The extracted anchor candidates (`links` mode), for auditing/observability. */
  candidates?: LinkCandidate[];
  /** Present only on failure. */
  error?: string;
}

// ---------------------------------------------------------------------------
// stdio helpers.
// ---------------------------------------------------------------------------

/** Read a single line (the task JSON) from stdin. Resolves on the first newline,
 *  or on EOF if the producer never sends one. */
function readTaskLine(): Promise<string> {
  return new Promise((resolve, reject) => {
    let buf = "";
    const onData = (chunk: Buffer) => {
      buf += chunk.toString("utf8");
      const nl = buf.indexOf("\n");
      if (nl !== -1) {
        cleanup();
        resolve(buf.slice(0, nl));
      }
    };
    const onEnd = () => {
      cleanup();
      resolve(buf);
    };
    const onErr = (err: Error) => {
      cleanup();
      reject(err);
    };
    const cleanup = () => {
      process.stdin.off("data", onData);
      process.stdin.off("end", onEnd);
      process.stdin.off("error", onErr);
    };
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", onData);
    process.stdin.on("end", onEnd);
    process.stdin.on("error", onErr);
  });
}

/** Emit exactly one JSON result line on stdout. All human/debug logging goes to
 *  stderr so stdout stays a clean single-line channel for the Rust host. */
function emitResult(result: TaskResult): void {
  process.stdout.write(`${JSON.stringify(result)}\n`);
}

function log(...args: unknown[]): void {
  process.stderr.write(`[browser-agent] ${args.map(String).join(" ")}\n`);
}

// ---------------------------------------------------------------------------
// Deterministic link mode: DOM anchor extraction + navigate-by-href (no LLM).
// ---------------------------------------------------------------------------

/** Tokens ignored when scoring a text match so the query's content words drive it. */
const MATCH_STOPWORDS = new Set([
  "the", "a", "an", "to", "and", "of", "on", "in", "for", "with", "this",
  "that", "please", "click", "open", "play", "go", "goto", "result", "results",
  "link", "links", "first", "second", "third", "top", "select", "choose",
]);

/**
 * Enumerate the active tab's anchor links via the DOM. Runs in page context
 * (equivalent to CDP `Runtime.evaluate` over `document.querySelectorAll('a')`),
 * returning only http/https anchors with a resolved absolute href, in DOM order.
 */
async function extractLinks(page: {
  evaluate: <T>(fn: () => T) => Promise<T>;
}): Promise<LinkCandidate[]> {
  const raw = await page.evaluate<Array<{ text: string; href: string }>>(() => {
    const anchors = Array.from(document.querySelectorAll("a")) as HTMLAnchorElement[];
    return anchors
      .map((a) => ({
        // `a.href` is the browser-resolved absolute URL; `textContent` is the
        // visible label, whitespace-collapsed.
        text: (a.textContent ?? "").replace(/\s+/g, " ").trim(),
        href: a.href,
      }))
      .filter((l) => /^https?:/i.test(l.href));
  });
  return raw.map((l, i) => ({ index: i + 1, text: l.text, href: l.href }));
}

/** Best text match: the candidate whose text contains the most query content
 *  words. Returns `undefined` when nothing overlaps. */
function bestTextMatch(
  candidates: LinkCandidate[],
  query: string,
): LinkCandidate | undefined {
  const tokens = (query.toLowerCase().match(/[a-z0-9]+/g) ?? []).filter(
    (t) => t.length > 1 && !MATCH_STOPWORDS.has(t),
  );
  if (tokens.length === 0) return undefined;
  let best: LinkCandidate | undefined;
  let bestScore = 0;
  for (const c of candidates) {
    const text = c.text.toLowerCase();
    let score = 0;
    for (const t of tokens) if (text.includes(t)) score += 1;
    if (score > bestScore) {
      bestScore = score;
      best = c;
    }
  }
  return bestScore > 0 ? best : undefined;
}

/** Resolve the task's `select` hint (or its intent) to one link candidate. */
function chooseLink(
  candidates: LinkCandidate[],
  task: Task,
): LinkCandidate | undefined {
  if (candidates.length === 0) return undefined;
  const sel = task.select;

  // Numeric select => Nth (1-based). `candidates[N-1]` is `undefined` when N is
  // out of range, which the caller reports as "no link at that position".
  if (typeof sel === "number" && Number.isFinite(sel)) {
    return candidates[Math.trunc(sel) - 1];
  }
  if (typeof sel === "string" && sel.trim() !== "") {
    const trimmed = sel.trim();
    if (/^\d+$/.test(trimmed)) {
      return candidates[Number.parseInt(trimmed, 10) - 1];
    }
    return bestTextMatch(candidates, trimmed);
  }

  // No usable select => best match against the intent, else the first link.
  return bestTextMatch(candidates, task.intent) ?? candidates[0];
}

/**
 * DETERMINISTIC path: extract the active tab's links and navigate directly to
 * the chosen href. No LLM, no Stagehand planning — the grounding is the DOM.
 */
async function runLinksMode(
  stagehand: Stagehand,
  task: Task,
): Promise<TaskResult> {
  const page = stagehand.context.activePage() ?? (await stagehand.context.newPage());

  if (task.url) {
    log(`goto ${task.url}`);
    await page.goto(task.url);
  }

  const candidates = await extractLinks(page);
  log(`extracted ${candidates.length} anchor link(s)`);

  const chosen = chooseLink(candidates, task);
  if (!chosen || !chosen.href) {
    return {
      ok: false,
      detail: `no matching link among ${candidates.length} candidate(s)`,
      error: "no_link_match",
      candidates,
    };
  }

  log(`navigate by href -> ${chosen.href}`);
  await page.goto(chosen.href);

  return {
    ok: true,
    detail: `navigated to ${chosen.text || chosen.href}`,
    chosenHref: chosen.href,
    candidates,
  };
}

// ---------------------------------------------------------------------------
// Main: one task per process.
// ---------------------------------------------------------------------------

async function main(): Promise<number> {
  const rawTask = (await readTaskLine()).trim();
  if (!rawTask) {
    emitResult({ ok: false, detail: "no task on stdin", error: "empty_input" });
    return 1;
  }

  let task: Task;
  try {
    task = JSON.parse(rawTask) as Task;
  } catch (err) {
    emitResult({ ok: false, detail: "task was not valid JSON", error: String(err) });
    return 1;
  }
  if (!task.intent || typeof task.intent !== "string") {
    emitResult({ ok: false, detail: "task.intent (string) is required", error: "missing_intent" });
    return 1;
  }
  const mode: TaskMode = task.mode === "links" ? "links" : "act";
  // Only the LLM-planned `act` path needs the key; `links` mode is deterministic.
  if (mode === "act" && !GEMINI_API_KEY) {
    emitResult({ ok: false, detail: "GEMINI_API_KEY is not set", error: "missing_api_key" });
    return 1;
  }

  // Explicit DevTools flags per the spike brief, even though `port` + `userDataDir`
  // below already imply them — keeps the "whose Chrome" contract obvious.
  const launchArgs = [`--remote-debugging-port=${CDP_PORT}`, `--user-data-dir=${USER_DATA_DIR}`];

  const stagehand = new Stagehand({
    env: "LOCAL",
    // Gemini plans every act()/observe() turn.
    model: { modelName: MODEL_NAME, apiKey: GEMINI_API_KEY },
    localBrowserLaunchOptions: {
      // When CDP_URL is set we ATTACH to a Chrome the host already launched;
      // otherwise Stagehand launches one on CDP_PORT with the FlowRad profile.
      ...(CDP_URL ? { cdpUrl: CDP_URL } : { port: CDP_PORT }),
      userDataDir: USER_DATA_DIR,
      preserveUserDataDir: true,
      headless: HEADLESS,
      args: launchArgs,
      ...(CHROME_PATH ? { executablePath: CHROME_PATH } : {}),
    },
    verbose: 1,
    // Route Stagehand's own logs to stderr so stdout stays the result channel.
    logger: (line) => log(line.category ?? "log", line.message),
  });

  const timeout = task.timeoutMs ?? DEFAULT_TIMEOUT_MS;

  try {
    log(`init: mode=${mode} env=LOCAL model=${MODEL_NAME} cdp=${CDP_URL || `port ${CDP_PORT}`} profile=${USER_DATA_DIR}`);
    await stagehand.init();

    // DETERMINISTIC path: extract links + navigate by href. No LLM planning.
    if (mode === "links") {
      const linkResult = await runLinksMode(stagehand, task);
      emitResult(linkResult);
      return linkResult.ok ? 0 : 1;
    }

    if (task.url) {
      log(`goto ${task.url}`);
      const page = stagehand.context.activePage() ?? (await stagehand.context.newPage());
      await page.goto(task.url);
    }

    // observe() first: proves the DOM is grounded and surfaces the concrete
    // candidate actions Stagehand sees. Non-fatal if it finds nothing.
    let observed: unknown[] = [];
    try {
      observed = await stagehand.observe(task.intent, { timeout });
      log(`observe found ${Array.isArray(observed) ? observed.length : 0} candidate action(s)`);
    } catch (err) {
      log(`observe failed (non-fatal): ${String(err)}`);
    }

    // act(): Stagehand plans with Gemini and executes precise CDP clicks/typing.
    const result = await stagehand.act(task.intent, { timeout });

    emitResult({
      ok: result.success,
      detail: result.message || result.actionDescription || "completed",
      actions: result.actions ?? observed,
    });
    return result.success ? 0 : 1;
  } catch (err) {
    emitResult({ ok: false, detail: "browser task threw", error: String(err) });
    return 1;
  } finally {
    try {
      await stagehand.close();
    } catch (err) {
      log(`close failed (ignored): ${String(err)}`);
    }
  }
}

main()
  .then((code) => process.exit(code))
  .catch((err) => {
    // Last-resort guard so the host always gets a result line.
    emitResult({ ok: false, detail: "fatal", error: String(err) });
    process.exit(1);
  });
