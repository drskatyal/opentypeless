use crate::app_detector::types::{
    AppStyleOverride, ArtifactKind, ContextFamily, Density, Formality, ListBehavior, MarkupPolicy,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextPolicy {
    pub artifact_kind: ArtifactKind,
    pub formality: Formality,
    pub density: Density,
    pub markup: MarkupPolicy,
    pub list_behavior: ListBehavior,
    pub sentence_completeness: bool,
    pub preserve_technical_tokens: bool,
    pub forbidden_additions: &'static [&'static str],
}

impl ContextPolicy {
    pub fn for_family(family: ContextFamily) -> Self {
        use ArtifactKind::*;
        use Density::*;
        use Formality::*;
        use ListBehavior::*;
        use MarkupPolicy::*;

        match family {
            ContextFamily::Email => Self {
                artifact_kind: Email,
                formality: Professional,
                density: Balanced,
                markup: PlainText,
                list_behavior: NumberWhenExplicit,
                sentence_completeness: true,
                preserve_technical_tokens: true,
                forbidden_additions: &["subject line", "recipient not spoken"],
            },
            ContextFamily::WorkChat => Self {
                artifact_kind: Message,
                formality: Neutral,
                density: Compact,
                markup: PlainText,
                list_behavior: LineBreaks,
                sentence_completeness: false,
                preserve_technical_tokens: true,
                forbidden_additions: &["status heading", "greeting", "sign-off"],
            },
            ContextFamily::PersonalChat => Self {
                artifact_kind: Message,
                formality: Casual,
                density: Compact,
                markup: PlainText,
                list_behavior: LineBreaks,
                sentence_completeness: false,
                preserve_technical_tokens: false,
                forbidden_additions: &["status heading", "business framing", "sign-off"],
            },
            ContextFamily::Document => Self {
                artifact_kind: Prose,
                formality: Neutral,
                density: Expanded,
                markup: Structured,
                list_behavior: NumberWhenExplicit,
                sentence_completeness: true,
                preserve_technical_tokens: true,
                forbidden_additions: &["title", "executive summary", "citation"],
            },
            ContextFamily::ProjectManagement => Self {
                artifact_kind: TaskUpdate,
                formality: Neutral,
                density: Compact,
                markup: Light,
                list_behavior: Preserve,
                sentence_completeness: true,
                preserve_technical_tokens: true,
                forbidden_additions: &["assignee", "deadline", "ticket field"],
            },
            ContextFamily::DeveloperCollaboration => Self {
                artifact_kind: DeveloperNote,
                formality: Neutral,
                density: Compact,
                markup: Light,
                list_behavior: Preserve,
                sentence_completeness: true,
                preserve_technical_tokens: true,
                forbidden_additions: &["code", "commit hash", "implementation detail"],
            },
            ContextFamily::PromptOrCode => Self {
                artifact_kind: Prompt,
                formality: Neutral,
                density: Balanced,
                markup: Structured,
                list_behavior: NumberWhenExplicit,
                sentence_completeness: true,
                preserve_technical_tokens: true,
                forbidden_additions: &["code", "requirements", "acceptance criteria"],
            },
            ContextFamily::Support => Self {
                artifact_kind: SupportReply,
                formality: Professional,
                density: Balanced,
                markup: PlainText,
                list_behavior: NumberWhenExplicit,
                sentence_completeness: true,
                preserve_technical_tokens: true,
                forbidden_additions: &["policy", "refund promise", "resolution guarantee"],
            },
            ContextFamily::Social => Self {
                artifact_kind: SocialPost,
                formality: Casual,
                density: Compact,
                markup: PlainText,
                list_behavior: Preserve,
                sentence_completeness: true,
                preserve_technical_tokens: false,
                forbidden_additions: &["hashtag", "emoji", "call to action"],
            },
            ContextFamily::General => Self {
                artifact_kind: Prose,
                formality: Neutral,
                density: Balanced,
                markup: Light,
                list_behavior: NumberWhenExplicit,
                sentence_completeness: true,
                preserve_technical_tokens: true,
                forbidden_additions: &["greeting", "heading", "sign-off"],
            },
        }
    }

    pub fn with_override(mut self, value: AppStyleOverride) -> Self {
        self.artifact_kind = value.artifact_kind;
        self.formality = value.formality;
        self.density = value.density;
        self.markup = value.markup;
        self.list_behavior = value.list_behavior;
        self
    }

    pub fn render_family_rules(&self, family: ContextFamily) -> String {
        let family_rule = match family {
            ContextFamily::Email => {
                "Email: produce an email body when there is enough content. Use a greeting when the recipient is spoken, concise body paragraphs, and a light closing when appropriate. Do not generate a subject unless explicitly requested."
            }
            ContextFamily::WorkChat => {
                "Work chat: keep it casual and concise while preserving the user's tone. Use short sentences or simple line breaks when helpful. No greeting or sign-off."
            }
            ContextFamily::PersonalChat => {
                "Personal chat: keep the user's casual voice and short-message rhythm; do not turn it into a status report."
            }
            ContextFamily::Document => {
                "Document: use coherent paragraphs. Use short headings or bullet points when the spoken structure has sections, takeaways, or multiple items."
            }
            ContextFamily::ProjectManagement => {
                "Project management: format as a compact update with bullets for progress, blockers, and next steps when spoken. Do not invent owners, deadlines, or ticket fields."
            }
            ContextFamily::DeveloperCollaboration => {
                "Developer collaboration: format as a concise review or engineering note. Use bullets for issue, impact, and suggestion when helpful. Preserve technical identifiers, API names, versions, paths, and error tokens exactly."
            }
            ContextFamily::PromptOrCode => {
                "Prompt or code surface: make the spoken request explicit and usable. Use compact bullets for goal, constraints, and output shape when implied, but never invent code or unstated requirements."
            }
            ContextFamily::Support => {
                "Support: write a clear, empathetic reply. Use short paragraphs or numbered steps when next actions are spoken. Do not invent policy, entitlement, refund, or resolution claims."
            }
            ContextFamily::Social => {
                "Social: keep the user's voice and make it readable as a short post. No hashtags, emoji, or calls to action unless spoken."
            }
            ContextFamily::General => {
                "General: lightly polish into directly usable prose without assuming an artifact type."
            }
        };
        format!(
            "{family_rule}\nUse {:?} density. Add none of: {}.",
            self.density,
            self.forbidden_additions.join(", ")
        )
    }

    pub fn render_override_rules(&self) -> String {
        format!(
            "Reviewed structured override: artifact={:?}; formality={:?}; density={:?}; markup={:?}; list_behavior={:?}. This changes presentation only, never facts or operation.",
            self.artifact_kind,
            self.formality,
            self.density,
            self.markup,
            self.list_behavior,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_policy_forbids_unspoken_social_and_support_content() {
        let social = ContextPolicy::for_family(ContextFamily::Social);
        assert!(social.forbidden_additions.contains(&"hashtag"));
        assert!(social.forbidden_additions.contains(&"emoji"));

        let support = ContextPolicy::for_family(ContextFamily::Support);
        assert!(support.forbidden_additions.contains(&"refund promise"));
        assert!(support.forbidden_additions.contains(&"policy"));
    }
}
