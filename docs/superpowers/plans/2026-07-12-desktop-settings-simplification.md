# Desktop Settings Simplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce default Settings density while preserving all P0/P1 behavior, make Scenes the place where app-family writing modes are visible and configurable, and close two packaged-desktop accessibility/state gaps.

**Architecture:** Keep existing panes and stores. Simplify each component through progressive disclosure, reuse persisted `family_scene_assignments` for app-family Scene choices, preserve exact app mappings as user-owned overrides, and add only the minimal persisted `system_scene_overrides` config field rather than a second mapping store. Component behavior is frozen with focused Vitest tests before each production edit.

**Tech Stack:** React 19, TypeScript 5.8, Tailwind CSS 4, react-i18next, Zustand, Tauri 2, Vitest 4, Testing Library, existing Lucide and dialog/menu patterns.

## Global Constraints

- Approved source: `docs/superpowers/specs/2026-07-12-desktop-settings-simplification-design.md`.
- Preserve Settings route, six pane destinations, current design tokens, and 10px-or-smaller radii.
- Preserve password APIs, account semantics, cloud/quota behavior, app detection, prompt composition, intent routing, shortcut persistence, translation persistence, dictionary transactions, and backup privacy.
- Do not add a 71-app settings gallery or persist raw process, bundle, executable, host, title, URL, page, dictated, or selected-text material.
- Expose supported app-family writing modes in Scenes, with representative app logos and compact Scene selectors.
- Keep exact app override creation reachable only from a safe detected context or existing user mapping.
- Use existing local app marks and Lucide icons only.
- Every new icon-only action has an accessible name and tooltip.
- Use TDD: every production behavior change follows a focused failing test.
- Do not modify or stage unrelated `docs/growth/` or `docs/images/` files.

---

### Task 1: Simplify Shortcut Rows And Remove Duplicate Permission UI

**Files:**
- Modify: `src/components/Settings/ShortcutBindingList.tsx`
- Modify: `src/components/Settings/GeneralPane.tsx`
- Modify: `src/components/Settings/__tests__/ShortcutBindingList.test.tsx`
- Modify: `src/components/Settings/__tests__/Settings.test.tsx`
- Modify: `src/i18n/locales/{de,en,es,fr,it,ja,ko,pt,ru,zh}.json`

**Interfaces:**
- Consumes: existing `ShortcutBinding[]`, `onChange`, conflict validation, special options, and top-level permission banner.
- Produces: immediate capture on add, one overflow menu per manageable saved binding, and no duplicate General permission CTA.

- [ ] **Step 1: Add failing shortcut presentation tests**

Add cases that assert a single required binding has no `Primary`, move, remove, or More controls; a two-binding list shows one `Primary` and one More action per manageable row; opening More exposes `Make primary`/`Remove`; and Add immediately renders `Press a key combination...` plus an explicit Cancel action.

```tsx
expect(screen.queryByText('Primary')).not.toBeInTheDocument()
expect(screen.queryByRole('button', { name: 'Move shortcut up' })).not.toBeInTheDocument()
fireEvent.click(screen.getByRole('button', { name: 'Add shortcut' }))
expect(screen.getByRole('button', { name: 'Press a key combination...' })).toBeInTheDocument()
expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument()
```

- [ ] **Step 2: Verify RED**

Run:

```bash
npx vitest run src/components/Settings/__tests__/ShortcutBindingList.test.tsx --reporter=verbose
```

Expected: failure because the existing component renders Primary and three management controls and Add creates a passive row.

- [ ] **Step 3: Implement progressive shortcut management**

Add `autoStart?: boolean` and `onCancel?: () => void` to `HotkeyRecorder`. Start recording on mount when `autoStart` is true. Replace ArrowUp/ArrowDown/Trash2 rows with a `MoreHorizontal` menu that can make a secondary binding primary or remove an allowed binding. Close on Escape/backdrop and restore trigger focus. Keep the first binding label only when `bindings.length > 1`.

- [ ] **Step 4: Add failing General permission test**

Assert the General pane does not render `settings.accessibilityPermission` or a second Grant Permission action when the global permission condition is unmet.

- [ ] **Step 5: Verify RED, then remove duplicate General permission state/UI**

Remove `checkAccessibilityPermission`, `requestAccessibilityPermission`, `waitForAccessibilityPermission`, local trust state, and the macOS Accessibility section from `GeneralPane`. Do not change the existing top-level banner.

- [ ] **Step 6: Verify GREEN**

Run:

```bash
npx vitest run src/components/Settings/__tests__/ShortcutBindingList.test.tsx src/components/Settings/__tests__/Settings.test.tsx --reporter=verbose
```

Expected: all focused tests pass.

### Task 2: Compact AI Polish And Translation

**Files:**
- Modify: `src/components/Settings/LlmPane.tsx`
- Modify: `src/components/Settings/ContextAdaptationApps.tsx`
- Modify: `src/components/Settings/TranslationTargets.tsx`
- Modify: `src/components/Settings/__tests__/LlmPane.test.tsx`
- Modify: `src/components/Settings/__tests__/TranslationTargets.test.tsx`
- Modify: `src/i18n/locales/{de,en,es,fr,it,ja,ko,pt,ru,zh}.json`

**Interfaces:**
- Consumes: existing polish/context/translation config and one-to-five target contract.
- Produces: shorter default toggle stack, accurate logo state, and compact single-target translation UI with disclosed management.

- [ ] **Step 1: Add failing AI Polish tests**

Assert model capability copy is absent, cleanup/translation helper paragraphs are absent, context/selected-text explanations remain, and the app group is disabled when either AI Polish or Context Adaptation is off.

```tsx
expect(screen.queryByText('Context and thought-aware support is best effort')).not.toBeInTheDocument()
expect(screen.getByLabelText('Apps adapted by context')).toHaveAttribute('aria-disabled', 'true')
```

- [ ] **Step 2: Verify RED**

Run:

```bash
npx vitest run src/components/Settings/__tests__/LlmPane.test.tsx --reporter=verbose
```

- [ ] **Step 3: Implement the shorter AI stack**

Remove model-capability state/effect/import and the status paragraph. Remove the cleanup and translation description paragraphs. Pass `disabled={!config.polish_enabled || !config.context_adaptation_enabled}` to `ContextAdaptationApps`; add `aria-disabled` and an off-state accessible label.

- [ ] **Step 4: Add failing translation progressive-disclosure tests**

Single target: no radio, move, remove, or management list; one selector plus Add. Multiple targets: active selector and `Manage languages`; management controls appear only after disclosure.

- [ ] **Step 5: Verify RED, then implement compact translation**

Move `TranslationTargets` directly under the Always Translate toggle. Rename `targetLanguage` values to localized `Translate to`. Add internal `managing` state; preserve `targets`, `active_target`, uniqueness, ordering, removal, and maximum five.

- [ ] **Step 6: Verify GREEN**

Run:

```bash
npx vitest run src/components/Settings/__tests__/LlmPane.test.tsx src/components/Settings/__tests__/TranslationTargets.test.tsx --reporter=verbose
```

### Task 3: Add Dictionary Maintenance Segments

**Files:**
- Modify: `src/components/Settings/DictionaryPane.tsx`
- Modify: `src/components/Settings/__tests__/DictionaryPane.test.tsx`
- Modify: `src/i18n/locales/{de,en,es,fr,it,ja,ko,pt,ru,zh}.json`

**Interfaces:**
- Consumes: existing dictionary/correction state and all existing commands.
- Produces: one active `words | corrections` maintenance surface while preserving shared search/import/export state.

- [ ] **Step 1: Add failing segment tests**

Assert Words is selected initially, correction form/list is absent, switching to Corrections hides the word form/list, and switching back preserves entered draft values.

- [ ] **Step 2: Verify RED**

Run:

```bash
npx vitest run src/components/Settings/__tests__/DictionaryPane.test.tsx --reporter=verbose
```

- [ ] **Step 3: Implement segmented rendering**

Add local `activeSection: 'words' | 'corrections'`, reuse `SegmentedControl`, keep search/import/export above it, and conditionally render the existing editors/lists without changing handlers or state.

- [ ] **Step 4: Verify GREEN**

Run the focused Dictionary suite and confirm import/export/edit tests still pass.

### Task 4: Add App Writing Modes To Scenes

**Files:**
- Modify: `src/components/Settings/ScenesPane.tsx`
- Modify: `src/components/Settings/__tests__/Settings.test.tsx`
- Modify: `src/stores/appStore.ts`
- Modify: `src-tauri/src/storage/mod.rs`
- Modify: `src/i18n/locales/{de,en,es,fr,it,ja,ko,pt,ru,zh}.json`

**Interfaces:**
- Consumes: existing automatic `ContextPolicy` family rules, explicit `active_scene`, exact app `mapped_scene_id`, serialized `family_scene_assignments`, and exact app override commands.
- Produces: `App Writing Modes` in Scenes with representative app logos, concrete default mode names, editable default scenes in `My Scenes`, custom Scene override selectors, exact app mapped Scene prompts still resolving, no global Scene activation UI, and `family_scene_assignments` affecting runtime Scene selection when there is no legacy active Scene or exact app override.

- [ ] **Step 1: Add failing App Writing Modes tests**

Assert Scenes shows `App Writing Modes`, semantic app-family rows, representative app logos such as Gmail/Apple Mail and Slack/Lark, and one selector per family. The default selector label must be a concrete default mode name, not `Automatic`. Default scenes must appear in `My Scenes`, support edit/reset, and existing `family_scene_assignments` must preselect the chosen custom Scene.

- [ ] **Step 2: Verify RED**

Run:

```bash
npx vitest run src/components/Settings/__tests__/Settings.test.tsx --reporter=verbose
```

Expected: failure because the old Scene cards still expose assignment summaries and setup.

- [ ] **Step 3: Implement App Writing Modes UI**

Add a top Scenes section that lists app families, local app logos, descriptions, and compact selectors. Use concrete default mode names as the default option. Put default modes in `My Scenes` as editable/resettable default scenes instead of a separate built-in library. Keep the 71-app gallery out of the UI.

- [ ] **Step 4: Add failing persistence tests**

Changing a family selector calls `setFamilySceneAssignment`, updates both `config` and `savedConfig` through `applyPersistedConfigPatch`, and does not leave the Settings dirty bar visible.

- [ ] **Step 5: Verify RED, then wire persistence**

Use existing `setFamilySceneAssignment` and `family_scene_assignments`; do not add a second mapping store. Keep exact app override creation in the last-context flow.

- [ ] **Step 6: Add failing Rust runtime test**

Assert `automatic_scene_prompt` still returns prompts for an exact app `mapped_scene_id`, still returns `None` when legacy active Scene data is set, and returns the family-assigned/default Scene when only `family_scene_assignments` or a default app mode matches the current family.

- [ ] **Step 7: Verify RED, then restore family Scene runtime fallback**

Keep `family_scene_assignments` serialization, sanitization, and command/API compatibility. Resolve automatic Scene prompt by exact app mapping first, then family assignment/default app mode, unless legacy active Scene data is set. Skip Polish Style when a scene prompt owns the output shape.

- [ ] **Step 8: Verify GREEN**

Run the focused Settings and Rust storage tests.

### Task 5: Trap Password Dialog Focus

**Files:**
- Modify: `src/components/AccountPage/PasswordDialog.tsx`
- Modify: `src/components/AccountPage/__tests__/AccountPage.password.test.tsx`

**Interfaces:**
- Consumes: existing dialog fields/actions and `returnFocusRef`.
- Produces: forward/backward focus containment without changing password submission.

- [ ] **Step 1: Add failing focus-cycle tests**

Open the dialog, Tab from Cancel and expect the first password field; Shift+Tab from the first field and expect Cancel. Confirm no background Account action receives focus.

- [ ] **Step 2: Verify RED**

Run:

```bash
npx vitest run src/components/AccountPage/__tests__/AccountPage.password.test.tsx --reporter=verbose
```

- [ ] **Step 3: Implement focus containment**

Add a form ref and extend the existing keydown handler. Query enabled focusable controls inside the dialog, wrap Tab/Shift+Tab at the boundaries, and preserve Escape/busy semantics.

- [ ] **Step 4: Verify GREEN**

Run the focused Account suite.

### Task 6: Full Regression And Packaged Visual QA

**Files:**
- Modify: `docs/2026-07-11-typeless-feature-completion-p0-p1-spec.md`

- [ ] **Step 1: Run frontend gates**

```bash
npm test -- --reporter=dot
npm run lint
npm run build
```

Expected: 36+ files and 320+ tests pass, lint has zero errors, production build passes.

- [ ] **Step 2: Run Rust gates**

```bash
cd src-tauri
cargo test -q
```

Expected: 434 tests pass.

- [ ] **Step 3: Build the latest debug app**

```bash
npm run tauri -- build --debug --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

- [ ] **Step 4: Run packaged UI QA**

At 900x700 and 720x480, capture and inspect General single/multiple shortcut states, AI/context on/off, App Writing Modes with representative app logos and concrete default mode names, exact app override reachability, single/multiple translation targets, both Dictionary segments, Scene activation/clearing, and the password Tab/Shift+Tab cycle. Do not submit a password, save test mappings, or change macOS permission.

- [ ] **Step 5: Update verification record and final diff checks**

Record exact test/build results, then run:

```bash
git diff --check
git status --short
```
