//! The planner: transcript + grounding packet -> validated [`ActionPlan`].
//!
//! Fast-path first (no network for fixed verbs); otherwise one injection-hardened
//! `generateContent` call via [`LlmClient`], with strict JSON validation and one
//! repair retry. TODO(act-phase1): the LLM planning path + validation are stubbed
//! here — the types/signature below are the frozen contract.

use std::sync::Arc;

use super::action::ActionPlan;
use super::grounding_packet::GroundingPacket;
use super::llm::LlmClient;

/// Where a plan came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanSource {
    FastPath,
    Llm,
}

/// Why planning failed.
#[derive(Debug)]
pub enum PlanError {
    Http(String),
    InvalidJson(String),
    Schema(String),
    DeniedByPolicy(String),
    Timeout,
    Empty,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::Http(m) => write!(f, "planner transport error: {m}"),
            PlanError::InvalidJson(m) => write!(f, "planner returned invalid JSON: {m}"),
            PlanError::Schema(m) => write!(f, "planner output failed validation: {m}"),
            PlanError::DeniedByPolicy(m) => write!(f, "planner output denied by policy: {m}"),
            PlanError::Timeout => write!(f, "planner timed out"),
            PlanError::Empty => write!(f, "planner produced no plan"),
        }
    }
}
impl std::error::Error for PlanError {}

/// One planning turn's input.
pub struct PlanRequest {
    pub transcript: String,
    pub packet: GroundingPacket,
    /// Short, trusted context (e.g. "last action: focused #/2"), never PHI.
    pub prior_context: Option<String>,
}

/// A successful plan plus where it came from.
pub struct PlanResult {
    pub plan: ActionPlan,
    pub source: PlanSource,
}

/// Turns transcripts into action plans.
pub struct Planner {
    llm: Arc<dyn LlmClient>,
    model: String,
    max_retries: u8,
}

impl Planner {
    pub fn new(llm: Arc<dyn LlmClient>, model: String) -> Self {
        Self {
            llm,
            model,
            max_retries: 1,
        }
    }

    /// Fast-path first (no network); otherwise plan via the LLM.
    pub async fn plan(&self, req: PlanRequest) -> Result<PlanResult, PlanError> {
        if let Some(plan) = super::fastpath::resolve(&req.transcript) {
            return Ok(PlanResult {
                plan,
                source: PlanSource::FastPath,
            });
        }
        // TODO(act-phase1): build the injection-hardened prompt, call the LLM with
        // a responseSchema, validate (paths must exist in the packet; destructive
        // intents must ask_user or be omitted), and retry once on failure.
        let _ = (
            &self.llm,
            &self.model,
            self.max_retries,
            &req.packet,
            &req.prior_context,
        );
        Err(PlanError::Schema(
            "LLM planning path not yet implemented".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::action::Action;
    use crate::act::llm::test_support::FixtureLlmClient;

    fn empty_packet() -> GroundingPacket {
        GroundingPacket {
            app_name: "App".into(),
            window_title: "W".into(),
            focused_path: None,
            elements: vec![],
        }
    }

    #[tokio::test]
    async fn fast_path_resolves_without_touching_the_llm() {
        // The fixture panics-by-exhaustion if called; a fast-path verb must not call it.
        let llm = Arc::new(FixtureLlmClient::new(vec![]));
        let planner = Planner::new(llm.clone(), "fast".into());
        let res = planner
            .plan(PlanRequest {
                transcript: "copy".into(),
                packet: empty_packet(),
                prior_context: None,
            })
            .await
            .unwrap();
        assert_eq!(res.source, PlanSource::FastPath);
        assert_eq!(res.plan.actions.len(), 1);
        assert!(matches!(res.plan.actions[0], Action::Key { .. }));
        assert_eq!(llm.call_count(), 0);
    }
}
