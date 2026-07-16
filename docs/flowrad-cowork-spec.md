# FlowRad Cowork — Multi-Agent Orchestration Track (Act mode)

Status: **spec / design** (not yet implemented). This is the "Act mode" endgame:
voice-dispatched, parallel, focused task agents driving CLI tools and apps on the
user's own machine, with the user's own logged-in sessions.

Scope of _this_ track: the **orchestrator**, a **CLI worker**, and a **Cursor
(VS Code) extension worker**, plus the **confirmation + kill-switch** model.
Browser-CDP workers, native-a11y workers, and the multi-agent dashboard are
adjacent tracks referenced but not fully specced here.

---

## 1. Goal

Let a user speak a task ("Codex: add tests for the auth module"; "Cursor: refactor
this file"; "run the deploy script") and have FlowRad dispatch it to the right
**worker**, which executes it **in parallel** with other workers, each on a
focused task, with hard safety rails. This is "local parallel subagents across
apps, by voice" — distinct from cloud subagents because it uses the machine's
real processes and the user's authenticated app sessions.

Non-goal (this track): arbitrary "operate any GUI app." Native GUI control is a
separate, later, a11y-based track constrained by single-input focus.

---

## 2. Architecture

```
 Voice → STT (existing) → task text
     → ORCHESTRATOR
         • parse intent → {worker target, task payload}
         • route to a WORKER (spawn or reuse)
         • track task lifecycle + subgoal checklist
         • enforce policy (risk tier, confirmation, allowlist)
         • aggregate status → dashboard
     → WORKERS (run in parallel, isolated):
         ├─ CLI worker            (subprocess + pty; e.g. codex, claude, aider)
         ├─ Cursor worker         (VS Code extension API over a local bridge)
         └─ [later] browser-CDP / native-a11y workers
     → DASHBOARD (per-worker task, status, live log, controls)
```

Everything below the STT line is new. The orchestrator is a Rust module in the
Tauri backend (`src-tauri/src/cowork/`), workers are trait objects, the dashboard
is a React surface.

### 2.1 Core traits (Rust)

```rust
trait Worker {
    fn id(&self) -> WorkerId;
    fn kind(&self) -> WorkerKind;            // Cli | Cursor | Browser | Native
    async fn start(&mut self, task: Task) -> Result<()>;
    async fn poll(&mut self) -> WorkerEvent;  // Output | AwaitingConfirm | Done | Error
    async fn send(&mut self, input: WorkerInput) -> Result<()>; // stdin / command
    async fn kill(&mut self) -> Result<()>;   // MUST be immediate + idempotent
}

struct Task {
    id: TaskId,
    worker_target: WorkerTarget,   // by name ("codex") or capability
    payload: String,               // the dictated instruction
    risk_hint: RiskTier,           // pre-classified; executor re-checks
    cwd: Option<PathBuf>,          // scoped working dir
    allowlist: Allowlist,          // commands/domains/paths this task may touch
}
```

---

## 3. Worker: CLI

The cleanest parallelism — a CLI agent is just a process.

- **Spawn** each worker as a subprocess in its **own pseudo-terminal (pty)** with a
  **scoped `cwd`** and a **sanitized env** (no ambient secrets it doesn't need).
- **Dispatch**: task payload → worker stdin.
- **Observe**: stream stdout/stderr → orchestrator → dashboard log. Detect
  completion via process exit or an agreed sentinel/idle-timeout.
- **Parallel**: N processes, no GUI, no focus contention.
- **Confirmation hook**: CLI agents that execute shell can do damage. Two options,
  strongest first:
  1. Run the agent under a **command broker**: the agent's shell calls are routed
     through a FlowRad shim that checks the allowlist and pauses on risky commands
     for confirmation. (Requires the agent to support a hook/approval mode — many
     coding CLIs do.)
  2. If no hook, run in a **restricted profile** (scoped cwd, no network unless
     granted, destructive-command denylist) and **require the human to have set the
     agent's own auto-approve OFF** for destructive ops.
- **Kill**: SIGKILL the process group (pty session) — immediate, idempotent.

Targets: `codex` (OpenAI Codex CLI), `claude` (Claude Code), `aider`, generic
shell runners.

---

## 4. Worker: Cursor (VS Code extension)

Cursor is Electron/VS Code. **Drive it via an extension, not by clicking.**

- Ship a **FlowRad companion VS Code/Cursor extension** that opens a **local
  bridge** (localhost WebSocket / named pipe, token-authenticated) to the FlowRad
  app.
- The orchestrator sends commands over the bridge; the extension runs them via the
  VS Code extension API **inside** Cursor (in-process, ~ms, structured):
  - insert/replace text, open files (`workspace`/`window` APIs),
  - run commands (`commands.executeCommand`) incl. Cursor AI (Composer/Chat) triggers,
  - read editor/selection/diff state,
  - report results back over the bridge.
- **Parallel**: multiple Cursor windows, each with the extension, each a worker on
  its own repo/task. No focus contention (extension runs in each window's ext host).
- **Why extension over CDP**: stable public API vs. attaching CDP to a shipped app
  that can disable remote-debugging or change across updates. CDP-attach is the
  **fallback** for surfaces the extension API can't reach.
- **Confirmation hook**: the extension surfaces a confirm prompt (or defers to the
  orchestrator's confirm UI) before running consequential commands (e.g. anything
  that writes files outside the workspace, runs tasks, or triggers git).
- **Kill**: bridge `abort` message → extension cancels in-flight command; worker
  marked dead. (Can't kill Cursor itself; we cancel _our_ operations.)

---

## 5. Confirmation model

Every action the orchestrator or a worker is about to take is classified:

| Tier | Examples | Default policy |
|---|---|---|
| **Safe** | read files, open app, insert dictated text, run a read-only command | auto |
| **Reversible** | edit a file (workspace has undo/VCS), switch window, run a build | auto, logged |
| **Consequential** | `git push`, deploy, publish, `rm`, network POST that mutates, spend money | **explicit confirm** |
| **Forbidden** | credential exfiltration, disabling FlowRad's own guards, touching paths outside allowlist | **blocked** |

Rules:
- Classification is done by the **deterministic executor**, never trusted from the
  LLM/agent output.
- **Confirmation is specific**, not a generic "yes": "Confirm `git push origin main`"
  — reduces accidental approval from background speech.
- Confirm UI shows: worker, the exact command/action, the target (repo/host/path),
  risk tier, and Approve / Deny / Always-allow-this-exact-command-in-this-session.
- **Injection resistance**: task text (from STT) and any content a worker reads
  (files, web pages, tool output) are **data**, never instructions that can raise
  privilege. The allowlist governs regardless of what any text says.

---

## 6. Kill-switch model

Multiple, layered, always available:

1. **Global stop** — a always-listening hotkey + a persistent dashboard button that
   **immediately** halts every worker (kill processes, abort bridge ops) and pauses
   dispatch. Must work even if the UI is busy (handled on a dedicated thread).
2. **Per-worker kill** — stop one agent without touching others.
3. **Dead-man / budget guards** — auto-halt a worker on: exceeding a wall-clock
   budget, a token/cost budget, a max-actions count, or a repeated-error loop
   (guards against agents-of-agents runaway).
4. **Confirmation backstop** — while any confirm dialog is open, workers requiring
   approval are paused; the mic does not treat ambient speech as approval.
5. **Panic revert** — best-effort: restore clipboard, rely on workspace/VCS undo;
   for consequential actions there is no auto-undo (that's why they're gated).

Kill must be **immediate and idempotent** — calling it twice is safe; a killed
worker never re-emits actions.

---

## 7. Agents-of-agents safety (the sharp edge)

FlowRad orchestrates agents (Codex/Cursor) that are themselves agents running
code. Specific mitigations:

- **No recursion without budget**: every worker has hard caps (time, cost, actions,
  spawn-depth). FlowRad does not let a worker spawn more workers implicitly.
- **Loop detection**: repeated identical/near-identical actions or errors → halt +
  surface to user.
- **Scoped blast radius**: per-task `cwd`, path allowlist, and (where enforceable)
  no-network-by-default; a worker touching another worker's scope is blocked.
- **Confirmation for irreversible**: push/deploy/publish/delete always gated,
  regardless of which agent requested it.
- **Full audit log**: timestamp, worker, task, action, risk tier, confirm event,
  result — append-only, local, redact obvious secrets.
- **Cost visibility**: live per-worker token/$ estimate in the dashboard; global cap.

---

## 8. Voice dispatch

- Existing STT produces the task text. A lightweight **router** maps it to a worker
  target (by named app — "Codex", "Cursor" — or capability) and extracts the payload.
- **Command vs dictation**: Cowork requires an explicit mode (a distinct hotkey or a
  wake token) so ordinary dictation is never interpreted as a command.
- Ambiguous target → orchestrator asks (dashboard prompt), never guesses on
  consequential actions.

---

## 9. Latency

Control layers are all structured + fast: pty stdio ~ms, extension bridge ~ms.
Dominant costs are STT and any LLM planning — but Cowork dispatch is often
**deterministic** ("send this text to worker N; run"), needing no planner at all.
Reserve LLM planning for genuinely ambiguous multi-step tasks.

---

## 10. Phased build plan

1. **Orchestrator core** + **CLI worker** (pty subprocess, scoped cwd, stream I/O,
   kill) + global stop + audit log. Smallest, highest-value, zero GUI risk.
2. **Confirmation UI** + risk classifier + allowlist + budget/dead-man guards.
3. **Cursor extension worker** (companion extension + local bridge).
4. **Dashboard** (parallel workers, live logs, per-worker kill, cost).
5. **Browser-CDP worker** + **native-a11y worker** (adjacent tracks).

Sequencing rationale: the CLI worker delivers real parallel-agents-by-voice with
the least risk and no dependency on GUI automation; safety rails come before more
powerful workers.

---

## 11. Open questions for council red-team

- **Agents-of-agents**: is the budget + loop-detection + allowlist + confirm-on-
  irreversible model sufficient, or are there escape hatches (e.g. an inner agent
  writing a script that a later step runs, bypassing the command broker)?
- **CLI command broker**: is a shim/approval-hook realistic across `codex`/
  `claude`/`aider`, or do we fall back to restricted-profile + agent auto-approve-off?
- **Cursor: extension vs CDP** — confirm extension API covers the needed surface
  (trigger Composer, read diffs, run tasks) and is more reliable than CDP-attach
  across Cursor updates. Where does the extension API fall short?
- **Kill immediacy** under a busy UI thread — is a dedicated stop thread + process-
  group SIGKILL enough, or do we need a separate supervisor process?
- **Injection** from tool output/files a worker reads — concrete failure modes.
