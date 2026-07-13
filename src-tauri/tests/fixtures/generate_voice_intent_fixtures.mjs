import { writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const outputDirectory = dirname(fileURLToPath(import.meta.url))
const enabledFlags = {
  draft_insert: true,
  rewrite_selection: true,
  translate_selection: true,
  search: true,
}

function flags(overrides = {}) {
  return { ...enabledFlags, ...overrides }
}

function fallback(mode, hasSelection) {
  if (mode === 'ask') {
    return {
      expectedKind: hasSelection ? 'ask_selection' : 'open_question',
      expectedPlacement: 'popup_answer',
    }
  }
  return {
    expectedKind: hasSelection ? 'ask_selection' : 'dictate_insert',
    expectedPlacement: hasSelection ? 'popup_answer' : 'insert_at_cursor',
  }
}

function corpusBuilder(prefix) {
  const cases = []
  let sequence = 0
  return {
    add({
      id,
      mode = 'dictate',
      locale,
      utterance,
      hasSelection = false,
      routeFlags = flags(),
      expectedKind,
      expectedPlacement,
      expectedProvider = null,
      expectedPayload = null,
      expectedFallbackReason = null,
      destructiveBlocker = false,
    }) {
      cases.push({
        id: `${prefix}-${String(++sequence).padStart(3, '0')}-${id}`,
        mode,
        locale,
        utterance,
        hasSelection,
        flags: routeFlags,
        expectedKind,
        expectedPlacement,
        expectedProvider,
        expectedPayload,
        expectedFallbackReason,
        destructiveBlocker,
      })
    },
    blocked({ id, mode = 'dictate', locale, utterance, hasSelection = false, reason }) {
      this.add({
        id,
        mode,
        locale,
        utterance,
        hasSelection,
        ...fallback(mode, hasSelection),
        expectedFallbackReason: reason,
        destructiveBlocker: true,
      })
    },
    cases,
  }
}

const providers = [
  ['Google', 'google'],
  ['YouTube', 'youtube'],
  ['Amazon', 'amazon'],
  ['GitHub', 'github'],
]

function buildEnglish() {
  const corpus = corpusBuilder('en')
  const locale = 'en'

  const mandatory = [
    ['draft-follow-up', "draft a follow-up email about tomorrow's launch", 'draft_insert', 'insert_at_cursor', "a follow-up email about tomorrow's launch", null],
    ['discuss-draft', 'I need to draft a follow-up email tomorrow', 'dictate_insert', 'insert_at_cursor', null, 'ambiguous'],
    ['negated-draft', 'do not draft this yet', 'dictate_insert', 'insert_at_cursor', null, 'negated'],
    ['reported-draft', 'she said "draft a reply"', 'dictate_insert', 'insert_at_cursor', null, 'quoted_or_reported'],
    ['compose-file', 'compose.yaml belongs in the config directory', 'dictate_insert', 'insert_at_cursor', null, 'code_or_identifier'],
  ]
  for (const [id, utterance, expectedKind, expectedPlacement, expectedPayload, reason] of mandatory) {
    corpus.add({
      id,
      locale,
      utterance,
      expectedKind,
      expectedPlacement,
      expectedPayload,
      expectedFallbackReason: reason,
      destructiveBlocker: reason !== null,
    })
  }
  corpus.add({
    id: 'rewrite-warmer',
    locale,
    utterance: 'make this warmer',
    hasSelection: true,
    expectedKind: 'rewrite_selection',
    expectedPlacement: 'replace_selection',
  })
  corpus.add({
    id: 'ask-selection',
    locale,
    utterance: 'what does this mean?',
    hasSelection: true,
    expectedKind: 'ask_selection',
    expectedPlacement: 'popup_answer',
  })
  corpus.add({
    id: 'youtube-search',
    mode: 'ask',
    locale,
    utterance: 'search React tutorials on YouTube',
    expectedKind: 'search',
    expectedPlacement: 'open_url',
    expectedProvider: 'youtube',
    expectedPayload: 'React tutorials',
  })
  corpus.blocked({
    id: 'discuss-search',
    locale,
    utterance: 'I should search for this later',
    reason: 'ambiguous',
  })

  const draftPrefixes = ['draft', 'write', 'compose', 'reply with']
  const draftPayloads = [
    'a launch update',
    'a concise thank-you note',
    'the customer follow-up',
    'an email confirming Tuesday',
    'a friendly reply saying yes',
    'a project status message',
    'three interview questions',
    'a short release announcement',
    'a reminder about the deadline',
    'a professional apology',
  ]
  for (const prefix of draftPrefixes) {
    for (const payload of draftPayloads) {
      corpus.add({
        id: `draft-${prefix.replaceAll(' ', '-')}`,
        mode: prefix === 'reply with' ? 'ask' : 'dictate',
        locale,
        utterance: `${prefix} ${payload}`,
        expectedKind: 'draft_insert',
        expectedPlacement: 'insert_at_cursor',
        expectedPayload: payload,
      })
    }
  }

  const rewriteCommands = [
    'rewrite this',
    'rephrase this',
    'make this shorter',
    'make this longer',
    'make this warmer',
    'make this friendlier',
    'make this more formal',
    'make this more concise',
    'fix the grammar',
    'fix the spelling',
    'format this as bullets',
    'turn this into a checklist',
  ]
  for (const command of rewriteCommands) {
    for (const suffix of ['', '.']) {
      corpus.add({
        id: 'rewrite-positive',
        locale,
        utterance: `${command}${suffix}`,
        hasSelection: true,
        expectedKind: 'rewrite_selection',
        expectedPlacement: 'replace_selection',
      })
    }
  }

  const translationCommands = [
    'translate this to English',
    'translate this into Spanish',
    'translate the selection to French',
    'translate the selection into Japanese',
    'translate this to Simplified Chinese',
    'translate this into German',
  ]
  for (const command of translationCommands) {
    for (const suffix of ['', '?']) {
      corpus.add({
        id: 'translate-positive',
        locale,
        utterance: `${command}${suffix}`,
        hasSelection: true,
        expectedKind: 'translate_selection',
        expectedPlacement: 'replace_selection',
      })
    }
  }

  const informationalCommands = [
    'summarize this',
    'explain this',
    'compare this with the previous paragraph',
    'what does this mean',
    'why is this important',
    'how does this work',
    'who is mentioned here',
    'where is the risk',
  ]
  for (const command of informationalCommands) {
    for (const suffix of ['', '?']) {
      corpus.add({
        id: 'selection-question',
        locale,
        utterance: `${command}${suffix}`,
        hasSelection: true,
        expectedKind: 'ask_selection',
        expectedPlacement: 'popup_answer',
      })
    }
  }

  const searchQueries = ['Rust ownership guide', 'React server components', 'Tauri global shortcuts']
  for (const [display, provider] of providers) {
    for (const query of searchQueries) {
      for (const utterance of [`search ${query} on ${display}`, `search ${display} for ${query}`]) {
        corpus.add({
          id: 'search-positive',
          mode: 'ask',
          locale,
          utterance,
          expectedKind: 'search',
          expectedPlacement: 'open_url',
          expectedProvider: provider,
          expectedPayload: query,
        })
      }
    }
  }

  const defaults = [
    'The launch is tomorrow morning',
    'I wrote the first draft yesterday',
    'Search quality matters for this feature',
    'Please keep the existing wording',
    'Module composition follows dependency order',
    'The reply arrived late',
    'Translation quality improved this week',
    'The rewrite strategy needs discussion',
    'A concise update would be useful',
    'The selected paragraph has three claims',
  ]
  for (const utterance of defaults) {
    for (const mode of ['dictate', 'ask']) {
      corpus.add({ id: 'default-safe', mode, locale, utterance, ...fallback(mode, false) })
    }
  }

  for (const [index, utterance] of [
    'draft an automatic language note',
    'write an automatic mode reply',
    'compose an automatic mode update',
    'reply with automatic language confirmed',
  ].entries()) {
    const payload = utterance.replace(/^(draft|write|compose|reply with)\s+/, '')
    corpus.add({
      id: `automatic-positive-${index}`,
      locale: 'automatic',
      utterance,
      expectedKind: 'draft_insert',
      expectedPlacement: 'insert_at_cursor',
      expectedPayload: payload,
    })
  }
  for (const utterance of [
    'draft 帮我写一封邮件',
    'write 幫我寫一封郵件',
    '起草 project update',
    'compose 搜索 Rust 在 Google',
  ]) {
    corpus.blocked({
      id: 'automatic-mixed-blocker',
      locale: 'automatic',
      utterance,
      reason: 'ambiguous',
    })
  }

  const negated = [
    'do not draft a reply',
    "don't write an email",
    'never compose this message',
    'not yet, draft the response',
    'please do not rewrite this',
    "please don't translate this to French",
    'do not search Rust on Google',
    'never search YouTube for this',
    'do not make this shorter',
    'not yet, fix the grammar',
  ]
  for (const utterance of negated) {
    for (const suffix of ['', ' please', ' today']) {
      corpus.blocked({
        id: 'negated-command',
        mode: utterance.includes('search') ? 'ask' : 'dictate',
        locale,
        utterance: `${utterance}${suffix}`,
        hasSelection: /rewrite|translate|shorter|grammar/.test(utterance),
        reason: 'negated',
      })
    }
  }

  const reported = [
    'she said "draft a reply"',
    'he says write a note',
    'they asked me to compose an email',
    'the phrase "rewrite this" appears here',
    'the word draft is a verb',
    'quote: translate this to French',
    'she said search React on Google',
    'he says make this warmer',
    'they asked me to fix the grammar',
    'quoted command tokens include compose',
  ]
  for (const utterance of reported) {
    for (const suffix of ['', ' yesterday', ' in the transcript']) {
      corpus.blocked({
        id: 'reported-command',
        mode: utterance.includes('search') ? 'ask' : 'dictate',
        locale,
        utterance: `${utterance}${suffix}`,
        hasSelection: /rewrite|translate|warmer|grammar/.test(utterance),
        reason: 'quoted_or_reported',
      })
    }
  }

  const identifiers = [
    'compose.yaml belongs in config',
    'search(query) returns a result',
    'draft_email is the function name',
    'rewriteThis handles formatting',
    'write_file should remain unchanged',
    'translate_selection is an enum',
    'make_this_shorter is a test id',
    'fixGrammar() is deprecated',
    'search-provider is a field',
    'reply_with_payload is required',
    'draft.md is a filename',
    'compose.json is generated',
    'search.ts exports a helper',
    'rewrite_this.rs compiles',
    'translateThisToEnglish is a symbol',
    'format_this_as() is not a command',
    'turn_this_into_list is snake case',
    'writeMessage() is a callback',
    'draft-v2.txt is archived',
    'search?q=test is a query fragment',
  ]
  for (const utterance of identifiers) {
    corpus.blocked({
      id: 'identifier-command',
      locale,
      utterance,
      reason: 'code_or_identifier',
    })
  }

  const discussed = [
    'I need to draft the reply tomorrow',
    'We should write a message later',
    'Can we compose the email after lunch',
    'I might reply with more details later',
    'The next step is to rewrite this section',
    'We discussed how to make this warmer',
    'I plan to translate this into French',
    'Maybe search React on Google tomorrow',
    'The team will fix the grammar next week',
    'I want to format this as bullets eventually',
  ]
  for (const utterance of discussed) {
    for (const suffix of ['', ' after review']) {
      corpus.blocked({
        id: 'mid-sentence-command',
        mode: utterance.includes('search') ? 'ask' : 'dictate',
        locale,
        utterance: `${utterance}${suffix}`,
        hasSelection: /rewrite|warmer|translate|grammar|format/.test(utterance),
        reason: 'ambiguous',
      })
    }
  }

  for (const utterance of ['draft', 'write', 'compose', 'reply with', 'draft:', 'write...', 'compose,', 'reply with:']) {
    corpus.blocked({ id: 'missing-payload', locale, utterance, reason: 'missing_payload' })
  }

  for (const utterance of [
    'draft a reply',
    'write a note',
    'compose an email',
    'search Rust on Google',
    'rewrite this',
    'translate this to French',
    'make this warmer',
    'fix the grammar',
    'search React on YouTube',
    'reply with yes',
    'format this as bullets',
    'turn this into a list',
  ]) {
    corpus.blocked({
      id: 'unsupported-locale',
      mode: utterance.includes('search') ? 'ask' : 'dictate',
      locale: 'fr',
      utterance,
      hasSelection: /rewrite|translate|warmer|grammar|format|turn/.test(utterance),
      reason: 'unsupported_locale',
    })
  }

  const disabledCases = [
    ['draft-disabled', 'dictate', 'draft a reply', false, { draft_insert: false }],
    ['rewrite-disabled', 'dictate', 'rewrite this', true, { rewrite_selection: false }],
    ['translate-disabled', 'dictate', 'translate this to French', true, { translate_selection: false }],
    ['search-disabled', 'ask', 'search Rust on Google', false, { search: false }],
  ]
  for (const [id, mode, utterance, hasSelection, override] of disabledCases) {
    corpus.add({
      id,
      mode,
      locale,
      utterance,
      hasSelection,
      routeFlags: flags(override),
      ...fallback(mode, hasSelection),
      expectedFallbackReason: 'feature_disabled',
      destructiveBlocker: true,
    })
  }

  return corpus.cases
}

function buildChinese({ traditional }) {
  const prefix = traditional ? 'zh-hant' : 'zh-hans'
  const locale = traditional ? 'zh-Hant' : 'zh-Hans'
  const corpus = corpusBuilder(prefix)
  const words = traditional
    ? {
        draft: ['寫一封', '起草', '幫我寫', '回覆說', '寫個'],
        rewrite: ['改寫這段', '潤色這段', '把這段寫得更簡潔', '精簡這段', '擴寫這段', '修正這段', '把這段改成條列'],
        translate: ['把這段翻譯成英文', '翻譯這段到日文', '將選取文字翻譯成法文'],
        info: ['總結這段', '解釋這段', '比較這段', '這段是什麼意思', '為什麼這樣寫', '怎麼理解這段', '誰提到了風險', '哪裡需要修改'],
        negations: ['不要', '別', '不用', '先別', '暫時不要', '不需要', '不要幫我'],
        reported: ['他說', '她說', '他們說', '原文是', '這句話是', '引用', '文件裡寫著'],
        searchVerb: '搜尋',
        selectWord: '選取',
      }
    : {
        draft: ['写一封', '起草', '帮我写', '回复说', '写个'],
        rewrite: ['改写这段', '润色这段', '把这段写得更简洁', '精简这段', '扩写这段', '修正这段', '把这段改成列表'],
        translate: ['把这段翻译成英文', '翻译这段到日文', '将选中文字翻译成法文'],
        info: ['总结这段', '解释这段', '比较这段', '这段是什么意思', '为什么这样写', '怎么理解这段', '谁提到了风险', '哪里需要修改'],
        negations: ['不要', '别', '不用', '先别', '暂时不要', '不需要', '不要帮我'],
        reported: ['他说', '她说', '他们说', '原文是', '这句话是', '引用', '文档里写着'],
        searchVerb: '搜索',
        selectWord: '选中',
      }

  const mandatory = traditional
    ? [
        ['draft-follow-up', '幫我寫一封明天發佈會的跟進郵件', 'draft_insert', 'insert_at_cursor', '一封明天發佈會的跟進郵件', null],
        ['discuss-draft', '我明天需要起草一封跟進郵件', 'dictate_insert', 'insert_at_cursor', null, 'ambiguous'],
        ['negated-draft', '不要幫我寫這封郵件', 'dictate_insert', 'insert_at_cursor', null, 'negated'],
        ['reported-draft', '他說「起草一封回覆」', 'dictate_insert', 'insert_at_cursor', null, 'quoted_or_reported'],
      ]
    : [
        ['draft-follow-up', '帮我写一封明天发布会的跟进邮件', 'draft_insert', 'insert_at_cursor', '一封明天发布会的跟进邮件', null],
        ['discuss-draft', '我明天需要起草一封跟进邮件', 'dictate_insert', 'insert_at_cursor', null, 'ambiguous'],
        ['negated-draft', '不要帮我写这封邮件', 'dictate_insert', 'insert_at_cursor', null, 'negated'],
        ['reported-draft', '他说“起草一封回复”', 'dictate_insert', 'insert_at_cursor', null, 'quoted_or_reported'],
      ]
  for (const [id, utterance, expectedKind, expectedPlacement, expectedPayload, reason] of mandatory) {
    corpus.add({
      id,
      locale,
      utterance,
      expectedKind,
      expectedPlacement,
      expectedPayload,
      expectedFallbackReason: reason,
      destructiveBlocker: reason !== null,
    })
  }
  corpus.add({
    id: 'translate-selection',
    locale,
    utterance: traditional ? '把這段翻譯成英文' : '把这段翻译成英文',
    hasSelection: true,
    expectedKind: 'translate_selection',
    expectedPlacement: 'replace_selection',
  })
  corpus.add({
    id: 'github-search',
    mode: 'ask',
    locale,
    utterance: traditional ? '在 GitHub 搜尋 tauri global shortcut' : '在 GitHub 搜索 tauri global shortcut',
    expectedKind: 'search',
    expectedPlacement: 'open_url',
    expectedProvider: 'github',
    expectedPayload: 'tauri global shortcut',
  })

  const payloads = traditional
    ? ['明天的跟進郵件', '一則簡短通知', '確認週二的回覆', '專業的道歉信', '發佈進度更新', '三個訪談問題', '友善的感謝訊息', '同意方案的回覆']
    : ['明天的跟进邮件', '一条简短通知', '确认周二的回复', '专业的道歉信', '发布进度更新', '三个访谈问题', '友好的感谢消息', '同意方案的回复']
  for (const draftPrefix of words.draft) {
    for (const payload of payloads) {
      corpus.add({
        id: 'draft-positive',
        locale,
        utterance: `${draftPrefix}${payload}`,
        expectedKind: 'draft_insert',
        expectedPlacement: 'insert_at_cursor',
        expectedPayload: payload,
      })
    }
  }

  for (const command of words.rewrite) {
    for (const suffix of ['', '。']) {
      corpus.add({
        id: 'rewrite-positive',
        locale,
        utterance: `${command}${suffix}`,
        hasSelection: true,
        expectedKind: 'rewrite_selection',
        expectedPlacement: 'replace_selection',
      })
    }
  }
  for (const command of words.translate) {
    for (const suffix of ['', '。', '？', '！']) {
      corpus.add({
        id: 'translate-positive',
        locale,
        utterance: `${command}${suffix}`,
        hasSelection: true,
        expectedKind: 'translate_selection',
        expectedPlacement: 'replace_selection',
      })
    }
  }
  for (const command of words.info) {
    for (const suffix of ['', '？']) {
      corpus.add({
        id: 'selection-question',
        locale,
        utterance: `${command}${suffix}`,
        hasSelection: true,
        expectedKind: 'ask_selection',
        expectedPlacement: 'popup_answer',
      })
    }
  }

  const queries = traditional
    ? ['Rust 所有權教學', 'React 元件測試', 'Tauri 全域快捷鍵']
    : ['Rust 所有权教程', 'React 组件测试', 'Tauri 全局快捷键']
  for (const [display, provider] of providers) {
    for (const query of queries) {
      for (const utterance of [`在 ${display} ${words.searchVerb} ${query}`, `${words.searchVerb} ${query} 在 ${display}`]) {
        corpus.add({
          id: 'search-positive',
          mode: 'ask',
          locale,
          utterance,
          expectedKind: 'search',
          expectedPlacement: 'open_url',
          expectedProvider: provider,
          expectedPayload: query,
        })
      }
    }
  }

  const defaults = traditional
    ? ['明天早上發佈', '昨天完成了初稿', '搜尋品質很重要', '保留現在的措辭', '回覆稍後才到', '翻譯品質有所提升', '需要討論改寫策略', '這段有三個觀點', '先確認需求', '今天不做變更']
    : ['明天早上发布', '昨天完成了初稿', '搜索质量很重要', '保留现在的措辞', '回复稍后才到', '翻译质量有所提升', '需要讨论改写策略', '这段有三个观点', '先确认需求', '今天不做变更']
  for (const utterance of defaults) {
    for (const mode of ['dictate', 'ask']) {
      corpus.add({ id: 'default-safe', mode, locale, utterance, ...fallback(mode, false) })
    }
  }

  for (const [index, command] of words.draft.slice(0, 4).entries()) {
    const payload = payloads[index]
    corpus.add({
      id: `automatic-positive-${index}`,
      locale: 'automatic',
      utterance: `${command}${payload}`,
      expectedKind: 'draft_insert',
      expectedPlacement: 'insert_at_cursor',
      expectedPayload: payload,
    })
  }
  for (const utterance of traditional
    ? ['draft 幫我寫一封郵件', 'write 寫個通知', '起草 project update', '幫我写一封混合文字郵件']
    : ['draft 帮我写一封邮件', 'write 写个通知', '起草 project update', '帮我寫一封混合文字邮件']) {
    corpus.blocked({
      id: 'automatic-mixed-blocker',
      locale: 'automatic',
      utterance,
      reason: 'ambiguous',
    })
  }

  const destructiveCommands = [
    `${words.draft[0]}回覆`,
    `${words.draft[1]}郵件`,
    words.rewrite[0],
    words.rewrite[1],
    words.translate[0],
    `${words.searchVerb} Rust 在 Google`,
    words.rewrite[3],
  ]
  for (const negation of words.negations) {
    for (const command of destructiveCommands.slice(0, 4)) {
      const utterance = `${negation}${command}`
      corpus.blocked({
        id: 'negated-command',
        locale,
        utterance,
        hasSelection: words.rewrite.some((value) => utterance.includes(value.slice(0, 2))),
        reason: 'negated',
      })
    }
  }

  for (const report of words.reported) {
    for (const command of destructiveCommands.slice(0, 4)) {
      const utterance = `${report}「${command}」`
      corpus.blocked({
        id: 'reported-command',
        locale,
        utterance,
        hasSelection: command.includes(traditional ? '改寫' : '改写') || command.includes(traditional ? '潤色' : '润色'),
        reason: 'quoted_or_reported',
      })
    }
  }

  const identifiers = traditional
    ? ['compose.yaml 是設定檔', 'search(query) 是函式', 'draft_email 是欄位', 'rewriteThis 是方法', 'write_file 不要改', 'translate_selection 是列舉', 'make_this_shorter 是測試名稱', 'fixGrammar() 已棄用', 'search-provider 是欄位', 'reply_with_payload 是參數']
    : ['compose.yaml 是配置文件', 'search(query) 是函数', 'draft_email 是字段', 'rewriteThis 是方法', 'write_file 不要改', 'translate_selection 是枚举', 'make_this_shorter 是测试名称', 'fixGrammar() 已弃用', 'search-provider 是字段', 'reply_with_payload 是参数']
  for (const utterance of [...identifiers, ...identifiers.map((value) => `${value} v2`)]) {
    corpus.blocked({ id: 'identifier-command', locale, utterance, reason: 'code_or_identifier' })
  }

  const discussed = traditional
    ? ['我明天需要起草郵件', '我們稍後再寫一封回覆', '團隊會改寫這段', '下週翻譯這段', '也許明天在 Google 搜尋', '先討論如何潤色這段', '之後再精簡這段', '我想修正這段', '有人說要寫個通知', '計畫把這段改成條列']
    : ['我明天需要起草邮件', '我们稍后再写一封回复', '团队会改写这段', '下周翻译这段', '也许明天在 Google 搜索', '先讨论如何润色这段', '之后再精简这段', '我想修正这段', '有人说要写个通知', '计划把这段改成列表']
  for (const utterance of discussed) {
    for (const suffix of ['', traditional ? '，等評審後' : '，等评审后']) {
      corpus.blocked({
        id: 'mid-sentence-command',
        mode: utterance.includes(words.searchVerb) ? 'ask' : 'dictate',
        locale,
        utterance: `${utterance}${suffix}`,
        hasSelection: /改写|改寫|翻译|翻譯|润色|潤色|精简|精簡|修正/.test(utterance),
        reason: /有人说|有人說/.test(utterance) ? 'quoted_or_reported' : 'ambiguous',
      })
    }
  }

  for (const command of [...words.draft, ...words.draft.map((value) => `${value}：`)]) {
    corpus.blocked({ id: 'missing-payload', locale, utterance: command, reason: 'missing_payload' })
  }

  for (const utterance of [
    `${words.draft[0]}回覆`,
    `${words.draft[1]}郵件`,
    words.rewrite[0],
    words.rewrite[1],
    words.translate[0],
    `${words.searchVerb} Rust 在 Google`,
    words.rewrite[3],
    words.rewrite[4],
    words.rewrite[5],
    words.info[0],
    words.info[1],
    `${words.draft[4]}通知`,
  ]) {
    corpus.blocked({
      id: 'unsupported-locale',
      mode: utterance.includes(words.searchVerb) ? 'ask' : 'dictate',
      locale: 'fr',
      utterance,
      hasSelection: words.rewrite.includes(utterance) || words.translate.includes(utterance),
      reason: 'unsupported_locale',
    })
  }

  const disabledCases = [
    ['draft-disabled', 'dictate', `${words.draft[0]}回覆`, false, { draft_insert: false }],
    ['rewrite-disabled', 'dictate', words.rewrite[0], true, { rewrite_selection: false }],
    ['translate-disabled', 'dictate', words.translate[0], true, { translate_selection: false }],
    ['search-disabled', 'ask', `${words.searchVerb} Rust 在 Google`, false, { search: false }],
  ]
  for (const [id, mode, utterance, hasSelection, override] of disabledCases) {
    corpus.add({
      id,
      mode,
      locale,
      utterance,
      hasSelection,
      routeFlags: flags(override),
      ...fallback(mode, hasSelection),
      expectedFallbackReason: 'feature_disabled',
      destructiveBlocker: true,
    })
  }

  return corpus.cases
}

const corpora = {
  voice_intent_en: buildEnglish(),
  voice_intent_zh_hans: buildChinese({ traditional: false }),
  voice_intent_zh_hant: buildChinese({ traditional: true }),
}

for (const [name, cases] of Object.entries(corpora)) {
  if (cases.length < 250) {
    throw new Error(`${name} has only ${cases.length} cases`)
  }
  const blockerCount = cases.filter((entry) => entry.destructiveBlocker).length
  if (blockerCount < 100) {
    throw new Error(`${name} has only ${blockerCount} blockers`)
  }
  writeFileSync(join(outputDirectory, `${name}.json`), `${JSON.stringify(cases, null, 2)}\n`)
  process.stdout.write(`${name}: ${cases.length} cases, ${blockerCount} blockers\n`)
}
