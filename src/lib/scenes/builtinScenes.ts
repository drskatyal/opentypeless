export interface BuiltInScene {
  id: string
  source: 'builtin'
  nameKey: string
  descriptionKey: string
  promptTemplate: string
}

export const BUILTIN_SCENES: BuiltInScene[] = [
  {
    id: 'builtin_clean_dictation',
    source: 'builtin',
    nameKey: 'scenes.builtin.cleanDictation.name',
    descriptionKey: 'scenes.builtin.cleanDictation.description',
    promptTemplate:
      'Lightly clean the transcript for readability while preserving the speaker meaning, wording choices, and factual content. Do not add new information.',
  },
  {
    id: 'builtin_meeting_notes',
    source: 'builtin',
    nameKey: 'scenes.builtin.meetingNotes.name',
    descriptionKey: 'scenes.builtin.meetingNotes.description',
    promptTemplate:
      'Rewrite the transcript as concise meeting notes with clear bullets, decisions, and action items. Preserve factual content and do not invent details.',
  },
  {
    id: 'builtin_professional_email',
    source: 'builtin',
    nameKey: 'scenes.builtin.professionalEmail.name',
    descriptionKey: 'scenes.builtin.professionalEmail.description',
    promptTemplate:
      'Rewrite the transcript as a concise professional email body. Use a greeting when the recipient is spoken, clear body paragraphs, and a light closing when appropriate. Do not add facts or generate a subject unless requested.',
  },
  {
    id: 'builtin_support_reply',
    source: 'builtin',
    nameKey: 'scenes.builtin.supportReply.name',
    descriptionKey: 'scenes.builtin.supportReply.description',
    promptTemplate:
      'Rewrite the transcript as a helpful customer support reply. Acknowledge the issue, give clear next steps, and avoid promising anything not stated.',
  },
  {
    id: 'builtin_technical_explanation',
    source: 'builtin',
    nameKey: 'scenes.builtin.technicalExplanation.name',
    descriptionKey: 'scenes.builtin.technicalExplanation.description',
    promptTemplate:
      'Rewrite the transcript as a clear technical explanation. Preserve precise terms, organize the reasoning, and avoid oversimplifying important details.',
  },
  {
    id: 'builtin_code_comment',
    source: 'builtin',
    nameKey: 'scenes.builtin.codeComment.name',
    descriptionKey: 'scenes.builtin.codeComment.description',
    promptTemplate:
      'Rewrite the transcript as a concise code review comment or inline engineering note. Keep it specific, actionable, and respectful.',
  },
  {
    id: 'builtin_product_spec_notes',
    source: 'builtin',
    nameKey: 'scenes.builtin.productSpecNotes.name',
    descriptionKey: 'scenes.builtin.productSpecNotes.description',
    promptTemplate:
      'Rewrite the transcript as product spec notes with goals, requirements, edge cases, and open questions. Do not invent decisions that were not spoken.',
  },
]
