//! The built-in drawer — a curated pack of universal recipes shipped so the
//! Conductor works out of the box, before the user has recorded anything.
//!
//! Every seed is a *universal, deterministic* route: an app launch (Terminator
//! searches + opens by name), a universal keyboard shortcut, a Windows
//! `ms-settings:` / `shell:` deep link, or a web/​media URL — the building blocks
//! that behave the same on every machine. In-app work that needs the live
//! accessibility tree is a **branch**: the recipe carries a parameterized goal
//! that the planner solves against a fresh snapshot of the opened app.
//!
//! Seeds ship as [`FlowStatus::Smoke`] — statically checked here, but only
//! promoted to `Verified` after a real run on the user's machine. The pack is
//! embedded (parsed from `seeds.json` at first use) so the drawer is never empty.

use super::flow::FlowFile;

/// The embedded seed pack, validated by the test below at build time.
const SEED_JSON: &str = include_str!("seeds.json");

/// The built-in recipes. Parses the embedded pack; returns empty (never panics)
/// if it somehow fails to parse — a bad seed must never take the app down. The
/// test suite guarantees it parses, so that path is unreachable in a shipped build.
pub fn builtin_flows() -> Vec<FlowFile> {
    match serde_json::from_str::<Vec<FlowFile>>(SEED_JSON) {
        Ok(flows) => flows,
        Err(e) => {
            tracing::error!(error = %e, "built-in seed pack failed to parse; drawer starts empty");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::flow::{FlowKind, FlowStatus};
    use super::*;
    use std::collections::HashSet;

    /// The primitive verbs a leaf step may use (must match the flow runner).
    const KNOWN_ACTIONS: &[&str] = &[
        "launch",
        "uri",
        "focus_app",
        "key",
        "focus",
        "set_value",
        "invoke",
        "pick_result",
        "choose",
        "wait",
    ];

    #[test]
    fn seed_pack_parses_and_is_non_empty() {
        let flows = builtin_flows();
        assert!(
            flows.len() >= 20,
            "expected a substantial pack, got {}",
            flows.len()
        );
    }

    #[test]
    fn every_seed_is_smoke_tier_and_structurally_valid() {
        for f in builtin_flows() {
            assert!(!f.id.is_empty(), "a seed has no id");
            assert!(!f.name.is_empty(), "{} has no name", f.id);
            assert!(!f.description.is_empty(), "{} has no description", f.id);
            assert_eq!(
                f.status,
                FlowStatus::Smoke,
                "{} must ship as Smoke (never Verified before a real run)",
                f.id
            );
            match f.kind {
                FlowKind::Leaf => {
                    assert!(!f.steps.is_empty(), "leaf {} has no steps", f.id);
                    for s in &f.steps {
                        assert!(
                            KNOWN_ACTIONS.contains(&s.action.as_str()),
                            "{} has unknown action {:?}",
                            f.id,
                            s.action
                        );
                        // Value-carrying verbs must carry a value.
                        if matches!(s.action.as_str(), "launch" | "uri" | "key" | "focus_app") {
                            assert!(
                                s.value.as_deref().is_some_and(|v| !v.is_empty()),
                                "{} step {} ({}) needs a value",
                                f.id,
                                s.id,
                                s.action
                            );
                        }
                    }
                }
                FlowKind::Branch => {
                    assert!(
                        f.branch_context
                            .as_deref()
                            .is_some_and(|c| !c.trim().is_empty()),
                        "branch {} needs a branch_context",
                        f.id
                    );
                }
            }
        }
    }

    #[test]
    fn seed_ids_are_unique() {
        let flows = builtin_flows();
        let mut seen = HashSet::new();
        for f in &flows {
            assert!(seen.insert(f.id.clone()), "duplicate seed id {}", f.id);
        }
    }

    #[test]
    fn slot_tokens_used_in_steps_are_declared() {
        // Any {slot} referenced in a value/branch_context must be a declared slot,
        // so a seed can never render an unfilled token into the OS.
        for f in builtin_flows() {
            let declared: HashSet<&str> = f.slots.iter().map(|s| s.name.as_str()).collect();
            let mut texts: Vec<String> = f.steps.iter().filter_map(|s| s.value.clone()).collect();
            if let Some(ctx) = &f.branch_context {
                texts.push(ctx.clone());
            }
            for t in texts {
                for tok in slot_tokens(&t) {
                    assert!(
                        declared.contains(tok.as_str()),
                        "{} references undeclared slot {{{}}}",
                        f.id,
                        tok
                    );
                }
            }
        }
    }

    /// Extract `{name}` slot tokens (name before any `|filter`), identifier chars only.
    fn slot_tokens(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else { break };
            let inner = &after[..close];
            let name = inner.split('|').next().unwrap_or("").trim();
            if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                out.push(name.to_string());
            }
            rest = &after[close + 1..];
        }
        out
    }
}
