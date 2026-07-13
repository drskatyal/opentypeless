# Desktop Account and Context UI Refinement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the signed-in inline password form with a compact modal and show representative supported-app logos beside context adaptation.

**Architecture:** Extract password mutation presentation into a focused `PasswordDialog` that receives credential capability and delegates the existing async mutation callback. Add a static, noninteractive `ContextAdaptationApps` component backed by existing local assets; `LlmPane` owns placement while the app registry remains the behavioral source of truth.

**Tech Stack:** React 19, TypeScript, Tailwind CSS, react-i18next, Vitest, Testing Library, existing `AppLogo` and desktop dialog patterns.

## Global Constraints

- Preserve the existing Account and Settings routes and navigation.
- Use only existing design tokens, 10px-or-smaller card/dialog radii, and local WebP app marks.
- Password dialog maximum width is 380px and must support Escape, backdrop close, focus-on-open, and busy-state close prevention.
- Show exactly Gmail, Slack, Lark, WeChat, Google Docs, Notion, GitHub, and Cursor, followed by `+63`.
- The logo row is noninteractive, uses 16px marks, has app-name accessibility labels/tooltips, and dims when AI polish disables context adaptation.
- Do not change password APIs, auth state semantics, app detection, prompt composition, or cloud contracts.

---

### Task 1: Freeze the Password Modal Contract

**Files:**
- Create: `src/components/AccountPage/PasswordDialog.tsx`
- Modify: `src/components/AccountPage/__tests__/AccountPage.password.test.tsx`
- Modify: `src/components/AccountPage/index.tsx`

**Interfaces:**
- Consumes: `credentialCapability: 'present' | 'none'`, `loading: boolean`, translated labels, and `onSubmit(currentPassword: string | null, newPassword: string): Promise<void>`.
- Produces: `PasswordDialog({ credentialCapability, loading, returnFocusRef, onCancel, onSubmit })` and a collapsed Account security row that conditionally mounts the dialog.

- [x] **Step 1: Write the failing modal tests**

Add assertions that clicking the Account password action opens `role="dialog"`, leaves the security row in place, does not render password fields before the click, renders current password only for credential users, closes on Escape, and stays open when validation fails.

```tsx
expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
fireEvent.click(screen.getByRole('button', { name: 'Change password' }))
expect(screen.getByRole('dialog', { name: 'Change password' })).toBeInTheDocument()
expect(screen.getByLabelText('Current password')).toHaveFocus()
fireEvent.keyDown(window, { key: 'Escape' })
expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
```

- [x] **Step 2: Run the focused test and verify red**

Run: `npx vitest run src/components/AccountPage/__tests__/AccountPage.password.test.tsx --reporter=verbose`

Expected: FAIL because the current password fields render inline and there is no password dialog.

- [x] **Step 3: Implement `PasswordDialog` using the existing dialog pattern**

Use the same backdrop, border, shadow, spacing, font sizes, and footer order as `CreateCorrectionDialog`. On open, focus the current-password field for `present`, otherwise the new-password field. Keep all field state and validation inside the dialog, prevent Escape/backdrop close while submitting, and clear state after successful submission or cancellation.

```tsx
<div className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/25 px-5">
  <div className="fixed inset-0" onClick={loading ? undefined : onCancel} />
  <div role="dialog" aria-modal="true" aria-label={title}
    className="relative z-10 w-full max-w-[380px] rounded-[10px] border border-border bg-bg-primary shadow-float">
    {/* 14px title, compact fields, bordered footer */}
  </div>
</div>
```

- [x] **Step 4: Replace inline Account form with the modal trigger**

Keep the `Security` row and one `Change password` or `Set password` text action. Render `PasswordDialog` adjacent to the page root when open; pass the existing store mutation without changing token rotation or OAuth-only behavior.

- [x] **Step 5: Run focused tests and verify green**

Run: `npx vitest run src/components/AccountPage/__tests__/AccountPage.password.test.tsx --reporter=verbose`

Expected: all Account password tests pass.

### Task 2: Add the Representative Context-App Logo Line

**Files:**
- Create: `src/components/Settings/ContextAdaptationApps.tsx`
- Modify: `src/components/Settings/LlmPane.tsx`
- Modify: `src/components/Settings/__tests__/LlmPane.test.tsx`

**Interfaces:**
- Consumes: `disabled: boolean` and existing `AppLogo({ iconKey, family })`.
- Produces: `ContextAdaptationApps({ disabled })`, a noninteractive logo line with accessible names and a `+63` count.

- [x] **Step 1: Write the failing logo-line test**

Render `LlmPane` and assert all eight representative app names are exposed exactly once, `+63` is visible, and there are no links or buttons inside the logo line.

```tsx
const coverage = screen.getByLabelText('Apps adapted by context')
for (const name of ['Gmail', 'Slack', 'Lark', 'WeChat', 'Google Docs', 'Notion', 'GitHub', 'Cursor']) {
  expect(within(coverage).getByLabelText(name)).toBeInTheDocument()
}
expect(within(coverage).getByText('+63')).toBeInTheDocument()
expect(within(coverage).queryByRole('button')).not.toBeInTheDocument()
```

- [x] **Step 2: Run the focused test and verify red**

Run: `npx vitest run src/components/Settings/__tests__/LlmPane.test.tsx --reporter=verbose`

Expected: FAIL because the representative coverage line does not exist.

- [x] **Step 3: Implement the static app metadata and compact row**

Define the eight entries in the component with `iconKey`, `family`, and `label`. Wrap each existing `AppLogo` in a noninteractive `span` with `aria-label` and `title`; use a stable 16px slot and 6px gap. Add `+63` as an 11px tertiary label and apply `opacity-40` when disabled.

- [x] **Step 4: Place the row under the context-adaptation hint**

Render it inside the existing adaptation block at `ml-[52px]`, before the optional last-dictation context. Pass `disabled={!config.polish_enabled}`, add only the accessibility label to the ten existing locale files, and do not add an app-management action.

- [x] **Step 5: Run focused tests and verify green**

Run: `npx vitest run src/components/Settings/__tests__/LlmPane.test.tsx --reporter=verbose`

Expected: all LLM settings tests pass.

### Task 3: Stabilize and Re-verify the Packaged Desktop

**Files:**
- Modify: `vitest.config.ts`
- Modify: `src/lib/__tests__/desktop-auth-callback.test.ts`
- Modify: `docs/2026-07-11-typeless-feature-completion-p0-p1-spec.md`

**Interfaces:**
- Consumes: the existing 36-file frontend suite and local macOS debug bundle workflow.
- Produces: deterministic four-worker frontend tests and revision 2.1 verification evidence.

- [x] **Step 1: Preserve the verified test-runner stabilization**

Keep `maxWorkers: 4` in `vitest.config.ts` and the static `createDesktopAuthCallbackURL` import. This removes environment-worker starvation without increasing the five-second per-test timeout.

- [x] **Step 2: Run all desktop gates**

Run:

```bash
npm test -- --reporter=dot
npm run lint
npm run build
cd src-tauri && cargo test -q
```

Expected: 320 frontend tests and 434 Rust tests pass; lint has zero errors; production frontend build exits zero.

- [x] **Step 3: Build and launch the debug app bundle**

Run: `npm run tauri -- build --debug --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'`

Expected: `OpenTypeless.app` is produced under `src-tauri/target/debug/bundle/macos/` without requiring a local updater signing key.

- [x] **Step 4: Verify real desktop interactions**

Launch the generated bundle by absolute path. Verify Account shows one collapsed Security row, password action opens a centered dialog without page reflow, Cancel/Escape closes it, AI Polish shows eight 16px marks plus `+63`, and no clipping/overlap appears in the current window size. Do not submit a real password or change macOS permissions.

- [x] **Step 5: Commit the refinement**

```bash
git add docs/2026-07-11-typeless-feature-completion-p0-p1-spec.md \
  docs/superpowers/plans/2026-07-12-desktop-account-context-ui-refinement.md \
  vitest.config.ts \
  src/lib/__tests__/desktop-auth-callback.test.ts \
  src/components/AccountPage/index.tsx \
  src/components/AccountPage/PasswordDialog.tsx \
  src/components/AccountPage/PasswordField.tsx \
  src/components/AccountPage/__tests__/AccountPage.password.test.tsx \
  src/components/Settings/LlmPane.tsx \
  src/components/Settings/ContextAdaptationApps.tsx \
  src/components/Settings/__tests__/LlmPane.test.tsx \
  src/i18n/locales/{de,en,es,fr,it,ja,ko,pt,ru,zh}.json
git commit -m "fix: refine desktop account and context UI"
```
