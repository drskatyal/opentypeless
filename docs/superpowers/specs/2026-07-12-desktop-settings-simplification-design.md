# OpenTypeless Desktop Settings Simplification Design

Date: 2026-07-12
Status: Approved for implementation
Approval: User reviewed the running macOS bundle and instructed implementation on 2026-07-12
Scope: Existing desktop Settings and signed-in password dialog only

## 1. Goal

Make the existing P0/P1 desktop capabilities feel quiet, obvious, and native to the current OpenTypeless UI. Preserve every shipped behavior while reducing default copy, duplicated status UI, disabled controls, and power-user management chrome.

## 2. Product Principles

- Default screens explain the next decision, not the implementation.
- Advanced controls appear only when the user has advanced state to manage.
- One system condition has one primary call to action.
- App adaptation remains a restrained feature signal, not an app gallery.
- Existing routes, tokens, spacing, typography, 10px-or-smaller radii, local logos, and compact dialogs remain authoritative.
- This UI-only spec does not independently redefine password, account, cloud, quota, detector, prompt, intent, dictionary, shortcut, or backup contracts. Companion specs cover prompt, permission, and local-provider hardening; cloud-restore behavior is recorded in the branch handoff and implementation tests rather than defined by this UI spec.

## 3. General Settings

### 3.1 Shortcut Rows

- A role with one binding shows only its label, the shortcut recorder, and the add action.
- Do not show `Primary`, move arrows, or a disabled remove action for a single binding.
- With multiple bindings, label only the first binding `Primary`.
- Replace the three always-visible move/remove controls with one More menu per manageable binding.
- A secondary binding menu offers `Make primary` when applicable and `Remove` when allowed.
- Adding a shortcut immediately starts capture. Never insert a passive `—` row that requires a second click.
- The adding row has a visible Cancel icon/button. Escape remains a valid standalone shortcut and is not treated as cancel while recording.
- Global conflict and registration behavior remains unchanged.

### 3.2 Accessibility Permission

- The existing top application permission banner remains the only primary Grant Permission surface.
- Remove the duplicated macOS Accessibility section and second Grant Permission button from General.
- Keep platform and registration errors only when they add information beyond the top permission banner.

## 4. AI Polish

- Remove the always-visible `Context and thought-aware support is best effort` / certified status line from the main settings flow.
- Do not request model-capability status solely for this removed presentation.
- Remove helper paragraphs for `AI cleanup for dictation` and `Always translate output`.
- Keep one concise context-adaptation explanation because the behavior is not obvious.
- Keep the selected-text explanation because it communicates operation/token scope.
- The representative app-logo row is active only when both AI Polish and Context Adaptation are enabled.
- Disabled logo state remains visible but dimmed; its accessible label communicates that adaptation is off.

## 5. Translation

- Translation remains part of AI Polish because it serves Always Translate, the Translate shortcut, and capsule target switching.
- Rename the visible concept from `Target Language` to the shorter localized equivalent of `Translate to`.
- Place the compact target control directly beneath the Always Translate toggle.
- With one target, show one selector and an Add action. Do not show a radio, arrows, remove action, or a separate heavy section.
- With multiple targets, show the active target selector plus a `Manage languages` disclosure.
- Only the expanded management state shows per-target selectors, ordering, and removal.
- Preserve the one-to-five unique target contract and the active-target compatibility mirror.

## 6. Scenes And App Context

- Scenes must show how AI Polish adapts to app types. The top section is `App Writing Modes`, organized by semantic app family rather than by a 71-app gallery.
- Each app-family row shows representative local app logos, the family label, a short example description, and a compact Scene selector.
- The selector's default option is a concrete default mode name such as `Email Format` or `Work Chat`, not `Automatic`.
- Built-in default modes live in the app-family rows and are not repeated as a separate built-in Scene library below.
- Built-in default modes also appear in `My Scenes` as editable default scenes with a `Reset to default` action.
- Selecting a custom Scene stores a family assignment and replaces the system mode for that app family.
- Editing a default scene stores only that scene's prompt override and immediately changes the default behavior for its app family.
- Existing `family_scene_assignments` is the backing store and runtime input for app-family Scene choices.
- Exact app overrides remain the user-owned path for a specific unknown/custom app or a specific app writing style.
- Custom Scenes remain reusable presets that can be assigned to app families. The UI no longer exposes global Scene activation.
- Legacy `active_scene` data remains runtime-compatible, but new UI should not encourage global Scene overrides.
- AI Polish retains the last-detected-app path for creating an exact app override when there is a safe local detection candidate or an existing user mapping.
- Exact app override UI does not show raw bundle IDs, URLs, titles, matcher material, or a 71-app gallery.

## 7. Dictionary

- Add a compact segmented control for `Words` and `Corrections`.
- Show only the active editor and list.
- Search, import, export, editing, transactional import, and correction behavior remain shared and unchanged.
- Switching segments clears no input or data.

## 8. Password Dialog

- Preserve the approved compact 380px modal and current validation/mutation behavior.
- Trap forward and backward keyboard focus inside the dialog.
- Disabled controls are skipped while the focus cycle remains inside active fields/actions.
- Escape, backdrop close, busy-state prevention, first-field focus, and trigger-focus restoration remain unchanged.

## 9. Accessibility And Layout

- No invisible focusable management controls.
- Every icon-only action has an accessible name and tooltip.
- Menus close on Escape and restore focus to their trigger.
- Dialogs remain within the 720x480 minimum viewport.
- Settings panes must not introduce horizontal overflow at 720x480 or 900x700.
- Existing reduced-motion and token behavior remains unchanged.

## 10. Verification

- New behavior is developed test-first with focused Vitest suites.
- Existing 36-file / 320-test frontend suite remains green.
- Existing 434 Rust tests remain green.
- `npm run lint`, `npm run build`, and the debug macOS app bundle must pass.
- Packaged-app QA covers General, AI Polish enabled/disabled states, App Writing Modes with concrete default mode names, exact app override reachability, Translation single/multiple states, Dictionary segments, Scene activation/clearing, and password focus cycling.
- No real password is submitted and no macOS permission is changed during QA.

## 11. Definition Of Done

- General no longer shows repeated Primary labels or disabled arrow/trash controls in the default single-binding state.
- The duplicate General accessibility CTA is removed.
- AI Polish default copy is materially shorter.
- Context logos accurately reflect the context-adaptation toggle.
- Translation is compact for the common one-language case.
- Scenes shows app-family writing modes with representative app logos and editable Scene choices.
- Family/default scene prompts change runtime Scene selection when no legacy active Scene or exact app override is set.
- Polish Style is skipped when an app/default/custom scene prompt owns the output shape.
- Exact custom app overrides remain available only from safe detected context or existing user mappings.
- Dictionary shows one maintenance mode at a time.
- Password focus cannot escape behind the modal.
- Focused, full frontend, Rust, lint, build, and packaged visual gates pass.
