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

interface Task {
  /** Natural-language goal for this browser turn. */
  intent: string;
  /** Optional URL to navigate to before acting (e.g. open YouTube first). */
  url?: string;
  /** Optional per-task timeout override, milliseconds. */
  timeoutMs?: number;
}

interface TaskResult {
  ok: boolean;
  detail: string;
  /** Stagehand's structured actions, when the turn produced any. */
  actions?: unknown[];
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
  if (!GEMINI_API_KEY) {
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
    log(`init: env=LOCAL model=${MODEL_NAME} cdp=${CDP_URL || `port ${CDP_PORT}`} profile=${USER_DATA_DIR}`);
    await stagehand.init();

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
