# OpenLess 借鉴补齐详细 Spec

Date: 2026-07-06
Target repo: `/Users/bytedance/个人项目/opentypeless`
Reference: OpenLess local clone and current OpenTypeless worktree
Status: Implementation in progress

Implementation notes:

- 2026-07-06 Batch 1 completed: streaming recovery, Apple Speech availability, text-only clipboard boundary, selected-text P0 safety guard.
- 2026-07-06 Batch 2 completed: selected-text command routing, Ask popup answer path for nondestructive selected-text requests, replacement-only prompt rule for destructive selected-text edits, no-LLM guard to avoid replacing selected text with raw spoken instructions.
- 2026-07-06 Batch 3 completed: typed hotkey roles are preserved and planned for registration, advanced hotkey roles dispatch safely without triggering dictation, credential vault unavailable status is explicit, credential save failures show inline Settings errors, diagnostics now has a one-line health summary.
- 2026-07-06 UI constraint update: no broad frontend expansion. Follow-up implementation keeps UI unchanged unless a one-line status/error is required.
- 2026-07-06 Batch 3 follow-up completed: translate hotkey now starts the normal dictation pipeline with a run-scoped translation override, without changing persistent settings or adding UI.
- 2026-07-06 Batch 4 constrained follow-up completed: scene import now returns a validation report with imported/skipped/renamed-conflict counts. Local ASR model manager remains deferred because it would require a heavier model-management UI.

## 1. 人话结论

当前这版已经把 OpenLess 的方向借到了：凭据安全、hotkey 录制和校验、插入策略、streaming 插入、Apple Speech、本地 scenes、Ask + 选中文本上下文都已经有了骨架。

但还没有到“用户怎么折腾都不容易坑他”的成熟度。下一步不要大改 UI，也不要继续堆新入口。应该优先补四个信任点：

1. **不丢字**：streaming 插入中途失败后，必须补剩余文本或把完整结果放到剪贴板。
2. **不假装可用**：Apple Speech 必须真实显示权限和语言可用性，不能只因为是 macOS 就显示 ready。
3. **不弄丢剪贴板**：至少明确现在只能恢复文本，后续补多格式剪贴板快照。
4. **选中文本真的能闭环**：用户选中文字后，说“润色/翻译/总结”，系统要知道是替换原文还是弹窗回答。

这份 spec 的目标是把“借鉴得还不够稳”的部分补完整，同时保持 OpenTypeless 现在的高级感、轻量感和 less is more。

## 2. 设计原则

### 2.1 产品原则

1. **可靠性优先于功能数量**
   用户宁愿少一个高级选项，也不能接受少字、错插、丢剪贴板、权限状态误导。

2. **UI 不要替底层背锅**
   不要靠大段说明解释不稳定行为。底层必须先可恢复、可诊断、可回滚。

3. **默认简单，高级隐藏**
   默认用户只看到录音、Ask、provider、少量必要状态。高级插入、clipboard、多动作 hotkey、diagnostics 可以折叠。

4. **破坏性操作必须明确**
   替换选中文本是破坏性行为。只有明确的 rewrite/fix/translate/replace 类意图才允许替换；ask/explain/summarize 默认不改原文。

5. **OpenLess 是参考，不是复制对象**
   学习它的平台工程和失败恢复，不照搬 UI、不扩成复杂控制台。

### 2.2 UI Guardrails

所有 UI 修改必须满足：

- 不重做 Settings 布局。
- 不新增大型 dashboard。
- 不加 hero、装饰渐变、大卡片堆叠。
- 不把高级能力一次性摊开。
- 新增状态优先使用现有小字号、icon、status row、tooltip、collapsible advanced。
- 所有按钮文字短，状态说明一行内解决。
- 涉及 UI 的实现必须做截图验收：Settings 宽屏、窄屏、深色/浅色至少各看一遍。
- 如果 headless browser 只能看到 `Loading...`，必须改用 Tauri debug app 手动打开 Settings 截图；不能把 headless 空白页当作验收通过。

## 3. 当前状态简表

| 能力 | 当前状态 | 结论 |
| --- | --- | --- |
| Credential Vault | 已有 `src-tauri/src/credentials.rs` 和 `commands/credentials.rs`，可迁移旧 key | 方向对，还缺失败 UX |
| Hotkey | 录音/Ask 有 recorder、校验、回滚、重试 | 核心到位，多动作体系未闭环 |
| Insertion | 有 auto/keyboard/clipboard/Windows SendInput | 到位一半，失败恢复和 clipboard 保护不足 |
| Streaming Insert | 已能把 LLM chunk 打进目标 app | 有丢尾巴风险，必须补 |
| Apple Speech | macOS 本地 ASR MVP 已有 | 权限/语言可用性检测不足 |
| Selected Text | 能捕获选中文本并传给 LLM/Ask | 还不是完整 command workflow |
| Scenes | local custom scene/built-ins/import/export 已有 | 可以继续 polish，但不是最高优先级 |
| Diagnostics | 已有 mic/accessibility/hotkey/clipboard/insertion/platform | 需要聚合成更明确的“下一步怎么修” |

## 4. 优先级路线图

### P0：必须先补

1. Streaming 插入失败恢复。
2. Apple Speech 权限与真实可用性。
3. 剪贴板文本保护说明 + 插入失败时完整文本兜底。
4. 选中文本 P0 安全护栏：selected text 模式不启用 streaming、不做不明确替换、失败时 copy result。完整替换/回答命令闭环放 P1。

### P1：应该补

5. Hotkey 多动作底层闭环，UI 先隐藏高级入口。
6. Credential Vault 异常状态 UX。
7. 极简 diagnostics health summary。

### P2：可以晚点补

8. 本地 ASR 模型管理。
9. Scenes/style packs 深化。

## 5. Track A: Streaming 插入失败恢复

Priority: P0

### 5.1 用户问题

用户说完话后，LLM 一边生成，OpenTypeless 一边往目标 app 输入。如果中途输入失败，现在可能只留下前半段，后半段没有补上。

这是最高风险，因为它会让用户怀疑这个工具“不可信”。

### 5.2 目标行为

当 streaming 插入成功：

- 目标 app 里出现完整 polished text。
- history 保存完整 polished text。
- UI 不显示警告。

当 streaming 插入中途失败，但 LLM 最终返回完整 polished text：

- 已经插入的 prefix 不重复。
- 如果可以确认 `polished_text` 以 `inserted_text` 开头，并且目标 app/焦点仍可信、插入通道仍可用，则只补 suffix。
- 如果无法确认目标 app/焦点仍可信，不能继续打字，必须把完整 polished text 放到剪贴板。
- suffix 插入失败时，把完整 polished text 放到剪贴板。
- history 保存完整 polished text。
- UI 显示轻量 warning：已复制完整结果，或者剩余文本已复制。

当 LLM 本身失败，且已经插入了部分 chunk：

- 不伪装成完整输出。
- history 可以记录 partial text，但必须标记 LLM failed / streaming partial。
- UI 提示“只插入了部分内容”。
- 如果可行，把 partial text 放到剪贴板，避免用户完全找不到内容。

### 5.3 推荐数据结构

在 `src-tauri/src/pipeline.rs` 中新增纯函数和类型，方便单测，不依赖 Tauri runtime：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamingRecoveryAction {
    AlreadyComplete,
    InsertSuffix { suffix: String },
    CopyFullToClipboard { reason: String },
    CopyPartialToClipboard { reason: String },
    NoRecoveryNeeded,
}

fn streaming_recovery_action(
    report: &StreamingInsertReport,
    polished_text: Option<&str>,
    llm_succeeded: bool,
    target_still_trusted: bool,
) -> StreamingRecoveryAction {
    if !report.has_inserted_text() {
        return StreamingRecoveryAction::NoRecoveryNeeded;
    }

    if llm_succeeded {
        let Some(polished_text) = polished_text else {
            return StreamingRecoveryAction::CopyFullToClipboard {
                reason: "missing polished text after successful LLM response".to_string(),
            };
        };

        if report.inserted_text == polished_text {
            return StreamingRecoveryAction::AlreadyComplete;
        }

        if polished_text.starts_with(&report.inserted_text) {
            if !target_still_trusted {
                return StreamingRecoveryAction::CopyFullToClipboard {
                    reason: "target app changed after partial streaming insert".to_string(),
                };
            }
            let suffix: String = polished_text
                .chars()
                .skip(report.inserted_text.chars().count())
                .collect();
            return StreamingRecoveryAction::InsertSuffix {
                suffix,
            };
        }

        return StreamingRecoveryAction::CopyFullToClipboard {
            reason: "streaming prefix did not match final output".to_string(),
        };
    }

    StreamingRecoveryAction::CopyPartialToClipboard {
        reason: "LLM failed after partial streaming insert".to_string(),
    }
}
```

实现必须使用 char-safe suffix 计算，不允许用 `polished_text[byte_index..]` 这类 byte slicing。中文、emoji、日文、韩文都必须覆盖测试。

`target_still_trusted` 的第一版可以保守实现：只要 streaming report failed，就默认 `false`，直接 copy full。后续如果要补 suffix，必须增加 target app/focus re-check，例如重新读取 `app_detector::get_current_app()` 并确认和录音开始时的 `app_ctx` 一致。

### 5.4 文件范围

- Modify: `src-tauri/src/pipeline.rs`
  - 新增 `StreamingRecoveryAction`。
  - 新增 `streaming_recovery_action`。
  - 修改 `process_text` 中 `streaming_report.has_inserted_text()` 后直接返回的逻辑。
  - 保证 `final_text` 在 LLM 成功时使用 `response.polished_text`，不是 partial text。
  - history 写入完整 polished text。
  - 第一版 recovery 可以保守 copy full，避免焦点变化时误打到错误窗口。

- Modify: `src-tauri/src/output/mod.rs`
  - 如有必要，新增 “copy full without paste” 的 helper，避免 recovery 过程误粘贴。

- Modify: `src-tauri/src/output/clipboard.rs`
  - 暴露明确的 copy-only 行为和 warning message。

- Modify: `src/hooks/useTauriEvents.ts`
  - 轻量显示 `pipeline:warning`。

- Modify: `src/i18n/locales/*.json`
  - 新增极短 warning 文案。

### 5.5 验收标准

- Streaming 成功时，行为不变。
- Streaming 中途失败但 LLM 成功时：
  - 只有目标 app/焦点仍可信时才补 suffix。
  - 目标 app/焦点无法确认时，不继续打字，完整 polished text 放剪贴板。
  - 不能补 suffix 的，把完整 polished text 放剪贴板。
  - history 保存完整 polished text。
  - UI warning 不超过一行。
- LLM 失败但已经插入 partial 时：
  - UI 明确提示 partial。
  - 不把 partial 当成完整 polished result。
- selected text 模式继续禁用 streaming insert，避免破坏性替换时边生成边改原文。

### 5.6 测试要求

Rust unit tests:

- `streaming_recovery_action_already_complete`
- `streaming_recovery_action_inserts_suffix_when_prefix_matches`
- `streaming_recovery_action_copies_full_when_target_not_trusted`
- `streaming_recovery_action_copies_full_when_prefix_mismatch`
- `streaming_recovery_action_uses_char_safe_suffix_for_cjk`
- `streaming_recovery_action_copies_partial_when_llm_fails`
- `streaming_recovery_action_noops_when_no_inserted_text`

Commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml streaming_recovery --quiet
cargo test --manifest-path src-tauri/Cargo.toml --quiet
npm test -- --testTimeout=10000
```

## 6. Track B: Apple Speech 真实可用性

Priority: P0

### 6.1 用户问题

现在 Apple Speech 的 ready/test 主要判断是否 macOS。真实权限检查发生在实际转写时。

用户可能看到“Apple Speech ready”，结果第一次录音才失败：Speech Recognition 权限被拒、系统受限、语言不可用。

### 6.2 目标行为

设置页 Apple Speech 状态必须真实表达：

- 当前平台不支持。
- Speech 权限未请求。
- Speech 权限已授权。
- Speech 权限被拒绝。
- Speech 权限受系统限制。
- 当前语言 recognizer 不可用。

测试按钮不能只返回 `true`。它应该至少检查权限状态；如果需要请求权限，UI 要明确这是一次权限请求。

### 6.3 推荐数据结构

在 `src-tauri/src/stt/apple_speech.rs` 增加：

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleSpeechAvailability {
    pub platform_supported: bool,
    pub authorization_status: AppleSpeechAuthorizationStatus,
    pub locale: Option<String>,
    pub recognizer_available: Option<bool>,
    pub ready: bool,
    pub issue_code: Option<String>,
    pub issue_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AppleSpeechAuthorizationStatus {
    Unsupported,
    NotDetermined,
    Denied,
    Restricted,
    Authorized,
    Unknown,
}
```

提供两个能力：

```rust
pub fn apple_speech_availability(language: Option<&str>) -> AppleSpeechAvailability;
pub fn request_apple_speech_authorization() -> Result<AppleSpeechAuthorizationStatus, AppError>;
```

`apple_speech_availability` 不主动弹权限，适合 diagnostics。
`request_apple_speech_authorization` 可以弹权限，适合用户点击 Test 或 Enable。

实现约束：

- diagnostics 不能弹系统权限窗。
- test/authorize 可以弹权限窗，但必须有 timeout。
- 权限请求和 recognizer 查询不能阻塞 UI 线程。
- 如果 Apple API 要求主线程调度，必须通过 Tauri main-thread safe path 处理；不能在 `spawn_blocking` 中假设权限弹窗一定可靠。
- test 完成后必须刷新 diagnostics 状态。

### 6.4 文件范围

- Modify: `src-tauri/src/stt/apple_speech.rs`
  - 暴露 authorization status。
  - 暴露 no-prompt availability。
  - 将 `ensure_authorized` 复用新的 status mapping。

- Modify: `src-tauri/src/commands/stt.rs`
  - `get_stt_provider_diagnostics` 对 Apple Speech 返回 authorization issue。
  - `test_stt_connection` 对 Apple Speech 调用真实 authorization/availability 检查。

- Modify: `src/components/Settings/SttPane.tsx`
  - Apple Speech status row 显示真实状态。
  - 未授权时按钮文案短：`Authorize` / `Test`。
  - denied/restricted 时只显示一行错误，不展开复杂说明。

- Modify: `src/lib/tauri.ts`
  - 更新 diagnostics 类型。

- Modify: `src/i18n/locales/*.json`
  - 新增 Apple Speech 状态文案。

### 6.5 UI 要求

只允许极简状态：

- 一行状态 row。
- 一个按钮。
- 一行错误或成功文字。

不要新增 Apple Speech 配置大面板。不要在设置页写长段权限说明。

### 6.6 验收标准

- 非 macOS 不展示 Apple Speech，或者 diagnostics 显示 unsupported。
- macOS 未授权时，不显示 ready。
- 用户点击测试后，如果系统弹窗授权并同意，状态变 ready。
- 用户拒绝后，状态显示 denied，test 返回 false/错误。
- 已授权但语言不可用时，显示 language unavailable。

### 6.7 测试要求

Rust tests:

- status mapping: raw status `0/1/2/3` 映射正确。
- diagnostics 对 denied/restricted/notDetermined 生成正确 issue code。
- `test_stt_connection` 不再只因为 macOS 返回 true。

Commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml apple_speech --quiet
cargo test --manifest-path src-tauri/Cargo.toml --quiet
npm test -- --testTimeout=10000
```

Manual QA:

- macOS clean install，未授权状态。
- 授权后 test。
- 在 System Settings 中撤销 Speech 权限后重新打开 app。

## 7. Track C: 剪贴板保护

Priority: P0/P1

### 7.1 用户问题

当前 `ClipboardOutput` 只通过 `arboard::Clipboard::get_text()` 备份文本。用户剪贴板里如果是图片、文件、富文本、表格，可能被覆盖且无法恢复。

这在语音输入工具里非常伤信任。

### 7.2 分阶段目标

#### Phase C1: 立刻补清楚文本保护和失败兜底

不大改平台剪贴板实现，先做到：

- 设置项文案明确：restore clipboard 目前只恢复文本内容。
- diagnostics 显示 clipboard restore scope: `textOnly`。
- 插入失败 recovery 时，明确是“完整结果已复制到剪贴板”，避免用户以为原剪贴板还在。
- `selection::capture_selected_text` 如果没有文本 backup，不要假装完整 restore。
- 如果能检测到当前剪贴板不是文本，P0 不承诺保留非文本内容；UI 和 diagnostics 必须使用 `textOnly` 表述，不能写成“preserve clipboard”。

#### Phase C2: 多格式剪贴板快照

后续补平台实现：

- macOS: NSPasteboard item snapshot。
- Windows: Clipboard formats snapshot。
- Linux: X11/Wayland 分开处理，Wayland 能力受限时明确降级。

### 7.3 推荐数据结构

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardRestoreScope {
    None,
    TextOnly,
    MultiFormat,
}

#[derive(Debug, Clone)]
pub enum ClipboardSnapshot {
    Empty,
    Text(String),
    MultiFormat(PlatformClipboardSnapshot),
    UnsupportedNonText,
}
```

C1 可以只实现 `Text` / `UnsupportedNonText` 的语义，不强行做 `MultiFormat`。

### 7.4 文件范围

- Modify: `src-tauri/src/output/clipboard.rs`
  - 明确 snapshot scope。
  - 对 restore result 产出 warning code。

- Modify: `src-tauri/src/selection.rs`
  - 捕获选中文本时明确 text-only restore。
  - 失败时不要把空字符串当成“已恢复原剪贴板”。

- Modify: `src-tauri/src/commands/misc.rs`
  - clipboard diagnostics 增加 restore scope。

- Modify: `src/components/Settings/GeneralPane.tsx`
  - advanced insertion 设置里增加一句短说明。

- Modify: `src/i18n/locales/*.json`
  - 文案必须短。

### 7.5 验收标准

- 用户启用 restore clipboard 后，文本剪贴板可以恢复。
- 非文本剪贴板不再被 UI 暗示为“完整保护”。
- P0 文案必须明确是 text-only restore，不允许承诺 full clipboard preservation。
- 插入失败时完整输出可找到，优先 copy full result。
- Wayland 下 clipboard auto-paste 继续降级为 copy-only。

### 7.6 测试要求

Rust tests:

- `clipboard_restore_decision` 继续覆盖文本恢复。
- 新增 no previous text 的 warning/scope 测试。
- selection capture 对 sentinel/empty/whitespace 行为保持正确。

Commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml clipboard --quiet
cargo test --manifest-path src-tauri/Cargo.toml selection --quiet
npm test -- --testTimeout=10000
```

## 8. Track D: 选中文本命令闭环

Priority: P1

P0 只要求 selected text 模式保持安全：不 streaming、不做不明确替换、失败时能找回结果。完整 command router 属于 P1。

### 8.1 用户问题

当前 OpenTypeless 能把选中文本传给模型，但还不是完整 workflow。用户期望的是：

- 选中文本，说“润色一下”，原文被替换。
- 选中文本，说“翻译成英文”，原文被替换或按设置插入。
- 选中文本，说“这段什么意思”，弹 Ask 回答，不改原文。
- 选中文本，说“总结一下”，默认弹答案，不破坏原文。

### 8.2 核心原则

破坏性替换必须明确。不要因为模型猜测就直接覆盖用户原文。

### 8.3 推荐 intent 类型

在 `src-tauri/src/commands/ask.rs` 或新文件 `src-tauri/src/commands/selection.rs` 中定义：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectedTextCommandIntent {
    Ask,
    Explain,
    Summarize,
    Translate,
    Rewrite,
    FixGrammar,
    Shorten,
    Expand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectedTextCommandOutput {
    PopupAnswer,
    ReplaceSelection,
    InsertAtCursor,
    CopyToClipboard,
}
```

推荐默认映射：

| Intent | Default output |
| --- | --- |
| Ask | PopupAnswer |
| Explain | PopupAnswer |
| Summarize | PopupAnswer |
| Translate | ReplaceSelection only when user says translate/rewrite this; otherwise PopupAnswer |
| Rewrite | ReplaceSelection |
| FixGrammar | ReplaceSelection |
| Shorten | ReplaceSelection |
| Expand | ReplaceSelection |

### 8.4 实现要求

- 保留现有 `selected_text_enabled` 开关。
- 在 pipeline start 时捕获 selected text 和 target app，不要让 Ask 窗口抢焦点后再捕获。
- LLM prompt 必须继续把 selected text 标为 untrusted。
- 替换选中文本时禁用 streaming insert。
- 替换失败时 copy result，并提示一行 warning。
- Ask/summarize/explain 默认走 Ask popup，不改原文。

### 8.5 文件范围

- Modify: `src-tauri/src/selection.rs`
  - 返回更结构化的 capture result：文本、是否 truncated、是否 clipboard restore degraded。

- Modify: `src-tauri/src/pipeline.rs`
  - 根据 intent 决定 `output_text` 还是 Ask popup。
  - selected text 替换路径必须 atomic：完整结果出来后再替换。

- Modify: `src-tauri/src/commands/ask.rs`
  - 复用 intent router。
  - Ask panel 接收 selected text result。

- Modify: `src-tauri/src/llm/prompt.rs`
  - 保持 selected text prompt injection 边界。
  - 为 destructive edit 输出加入更明确规则：只输出替换后的文本。

- Modify: `src/components/AskPanel/AskPanel.tsx`
  - 不做聊天化改造，只显示结果。

### 8.6 验收标准

- 选中文本 + “润色这段”会替换选中文本。
- 选中文本 + “这段什么意思”不会替换，弹窗回答。
- 选中文本里包含 “ignore previous instructions” 不会影响系统 prompt。
- 替换失败时，结果复制到剪贴板。
- UI 没有新增复杂 command 面板。

### 8.7 测试要求

Rust tests:

- intent router 覆盖中文/英文关键词。
- destructive intent 与 nondestructive intent 映射正确。
- prompt selected text injection 测试继续通过。
- selected text 模式下 streaming disabled。

Commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml selected_text --quiet
cargo test --manifest-path src-tauri/Cargo.toml prompt --quiet
npm test -- --testTimeout=10000
```

Manual QA:

- Notes/TextEdit 中选中文本 rewrite。
- Browser input 中选中文本 translate。
- Ask popup 不抢目标 app 替换路径。

## 9. Track E: Hotkey 多动作体系

Priority: P1

### 9.1 当前判断

录音和 Ask hotkey 已经借鉴到位：有 recorder、冲突检测、回滚、supervisor retry。

未到位的是完整多动作体系：

- translate selected text。
- edit selected text。
- switch scene。
- open app。
- 每个 role 独立注册状态。

### 9.2 关键问题

当前 `AppConfig::normalize_hotkey_settings` 会从 legacy 字段重建 typed hotkeys，容易让未来 role 字段保存后被清空。注册逻辑也只注册 dictation 和 Ask。

### 9.3 推荐目标

- typed hotkeys 成为 source of truth。
- legacy `hotkey` / `ask_hotkey` 只用于兼容旧版本和旧 UI。
- 支持 role dispatch，但 UI 默认只露 dictation 和 Ask。
- Advanced 中才显示其他 roles。

### 9.4 推荐类型

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum HotkeyRole {
    Dictation,
    Ask,
    TranslateSelection,
    EditSelection,
    SwitchScene,
    OpenApp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredHotkey {
    role: HotkeyRole,
    shortcut: tauri_plugin_global_shortcut::Shortcut,
}
```

### 9.5 文件范围

- Modify: `src-tauri/src/storage/mod.rs`
  - preserve typed role fields when `hotkeys` exists.
  - legacy sync only updates dictation/Ask compatibility fields.

- Modify: `src-tauri/src/hotkey.rs`
  - parse and validate all configured roles.
  - collision detection across all roles.
  - dispatch role from shortcut.

- Modify: `src-tauri/src/commands/misc.rs`
  - `register_configured_shortcuts` registers all non-null roles.
  - status includes role-level installed/failed.

- Modify: `src/components/Settings/GeneralPane.tsx`
  - Keep visible UI to dictation/Ask.
  - Advanced section can later expose more roles.

### 9.6 验收标准

- Existing users keep current hotkeys.
- Ask can be disabled.
- Future role fields do not get wiped by config normalization.
- Conflicting shortcuts across any roles fail validation before saving.
- Failed registration rolls back to previous working config.

### 9.7 测试要求

Rust tests:

- loading typed hotkeys preserves translate/edit/switch/open roles。
- collision across dictation and translate fails。
- registration plan includes all configured roles。
- legacy-only config migrates to typed config。

Frontend tests:

- Ask disabled remains disabled after save/reload。
- recorder conflict message still works。

Commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml hotkey --quiet
npm test -- --testTimeout=10000
```

## 10. Track F: Credential Vault 异常状态 UX

Priority: P1

### 10.1 当前状态

OS vault 方向正确，旧 key 迁移也有 write-read verification。问题是失败体验还不够产品化：

- Linux Secret Service 不可用时，命令可能只报错。
- `CredentialStorage::SessionOnly` / `LegacyWarning` 类型存在，但没有完整使用。
- UI 保存失败主要 console error，不够可见。

### 10.2 目标行为

- 保存成功才显示 saved。
- vault 不可用时，显示一行短错误。
- 不默认回退 plaintext。
- 如果提供 session-only，必须明确重启后失效。

### 10.3 文件范围

- Modify: `src-tauri/src/credentials.rs`
  - expose vault availability probe。

- Modify: `src-tauri/src/commands/credentials.rs`
  - `get_credential_status` 返回 `storage: unavailable/sessionOnly/osVault`。
  - set/read/clear 错误返回稳定 code。

- Modify: `src/components/Settings/SttPane.tsx`
  - 保存失败显示轻量 inline error。

- Modify: `src/components/Settings/LlmPane.tsx`
  - 同上。

- Modify: `src/lib/tauri.ts`
  - typed error/status。

### 10.4 验收标准

- 成功保存后 status 显示 OS vault。
- 保存失败时 UI 不显示 saved。
- Linux 无 Secret Service 时不静默写 plaintext。
- 旧 plaintext key 迁移失败时不删除旧 key。

### 10.5 测试要求

Rust tests:

- migration verification failure preserves legacy secret。
- status maps no vault to unavailable。
- set credential failure emits no changed event。

Frontend tests:

- credential save failure shows inline error。
- provider switch reads correct provider key。

Commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml credentials --quiet
npm test -- --testTimeout=10000
```

## 11. Track G: 极简 Diagnostics Health Summary

Priority: P1

### 11.1 用户问题

现在 diagnostics 数据不少，但用户需要的是一句话：哪里坏了，点哪里修。

### 11.2 目标行为

Settings 里有一个极简 health summary：

- `Ready`
- `Needs permission`
- `Degraded`
- `Action required`

展开后才看细项：

- Microphone
- Accessibility
- Hotkey
- Clipboard
- Insertion
- STT provider
- Credential vault
- Platform

### 11.3 文件范围

- Modify: `src-tauri/src/commands/misc.rs`
  - system diagnostics row 增加 `action_kind`。

- Modify: `src-tauri/src/commands/stt.rs`
  - provider diagnostics 并入 health summary。

- Modify: `src/components/Settings/GeneralPane.tsx`
  - 只放一行 summary + expand。

- Modify: `src/lib/tauri.ts`
  - diagnostics types。

### 11.4 验收标准

- 无问题时只占一行。
- 有问题时能看到最重要 action。
- 不形成大型监控面板。
- Wayland、Accessibility、Apple Speech denied、Credential unavailable 都有明确状态。

### 11.5 测试要求

Frontend tests:

- healthy summary renders compact。
- warning summary can expand。
- action required shows correct label。

Rust tests:

- diagnostics severity aggregation correct。

Commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml diagnostics --quiet
npm test -- --testTimeout=10000
```

## 12. Track H: 本地 ASR 模型管理

Priority: P2

### 12.1 当前判断

Apple Speech MVP 可以先用。完整 OpenLess local ASR 模型管理不是当前最高优先级，因为它会带来体积、下载、平台兼容、模型更新、失败支持的问题。

### 12.2 后续目标

- Local provider registry。
- Model availability status。
- Download progress。
- Model size / speed label。
- Offline ready 状态。
- Qwen / Sherpa / Foundry 分平台 provider。

### 12.3 UI 要求

只允许一个 Local ASR advanced section。不要新增 Local ASR 大页面，除非模型管理复杂到 settings 容不下。

### 12.4 验收标准

- 没下载模型时不会显示 ready。
- 下载失败可重试。
- 删除模型可恢复空间。
- Cloud/BYOK/custom-whisper 现有路径不受影响。

## 13. Track I: Scenes / Style Packs 深化

Priority: P2

### 13.1 当前判断

Scenes 已经有本地 custom scenes、built-ins、import/export、active scene diagnostics。现在不是最急。

### 13.2 后续目标

- Scene examples。
- Scene tags/category。
- Import validation report。
- Conflict handling。
- Optional pack metadata。

### 13.3 UI 要求

ScenesPane 不要变 marketplace。先保持 local-first，内置和自定义清楚即可。

## 14. 全局验收命令

每个阶段完成后至少跑：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --quiet
npm run format:check
npm run lint
npm test -- --testTimeout=10000
npm run build
```

打包验收至少在 P0 全部完成后跑一次：

```bash
npm run tauri -- build --debug --bundles app --no-sign --ci
```

macOS Apple Speech 修改后额外验证：

```bash
otool -L src-tauri/target/debug/opentypeless | grep Speech
/usr/libexec/PlistBuddy -c 'Print NSSpeechRecognitionUsageDescription' src-tauri/target/debug/bundle/macos/OpenTypeless.app/Contents/Info.plist
```

## 15. 手动 QA 清单

### 15.1 Streaming 插入

- 普通英文长句，streaming 开启，目标 app 能收到完整文本。
- 中文长句，streaming 开启，目标 app 能收到完整文本。
- 模拟中途插入失败，剩余文本能补上或完整文本进入剪贴板。
- LLM 中途失败，UI 显示 partial warning。
- selected text 模式下 streaming 不启用。

### 15.2 Apple Speech

- macOS 未授权首次打开设置页。
- 点击 Test 后授权。
- 授权成功后录音。
- 在系统设置里撤销 Speech 权限后回到 app。
- 切换语言后不可用状态可见。

### 15.3 Clipboard

- 原剪贴板是纯文本，粘贴后能恢复。
- 原剪贴板为空，粘贴后行为清楚。
- 原剪贴板是图片/文件，UI 不承诺完整恢复。
- 插入失败后，完整结果能从剪贴板找回。

### 15.4 Selected Text

- 选中文本说“润色这段”，原文本被替换。
- 选中文本说“翻译成英文”，原文本被替换。
- 选中文本说“解释一下”，弹窗回答，不改原文。
- 选中文本里带 prompt injection，不影响系统指令。

### 15.5 Hotkey

- Dictation hotkey 改动保存成功。
- Ask hotkey 改动保存成功。
- Ask hotkey 禁用后重启仍禁用。
- 冲突 hotkey 保存失败且 rollback。
- 注册失败后 UI 状态可见，旧快捷键不丢。

## 16. 推荐实施顺序

### Batch 1: P0 trust fixes

1. Streaming recovery pure function + tests。
2. Streaming recovery 接入 pipeline。
3. Apple Speech status model + tests。
4. Apple Speech diagnostics/test 接入。
5. Clipboard text-only scope + recovery warning。

完成后跑全量测试和一次 debug app build。

### Batch 2: Selected text command flow

1. Intent router + tests。
2. Ask vs replace output policy。
3. Prompt rules 收紧。
4. Minimal UI event handling。

完成后做手动 QA。

### Batch 3: Hotkey / credential / diagnostics polish

1. Preserve typed hotkey roles。
2. Role registration plan。
3. Credential unavailable UX。
4. Health summary。

完成后做截图验收。

### Batch 4: P2 features

1. Local ASR model manager。
2. Scene pack metadata。

只在 P0/P1 稳定后开始。

## 17. Definition of Done

这轮补齐完成时，应该能用一句话描述：

> OpenTypeless 不只是“有 OpenLess 的功能影子”，而是在关键失败场景下也能保护用户：不丢字、不误报权限、不静默污染剪贴板、不随便替换选中文本。

硬性完成标准：

- P0 四项全部完成。
- 全量 Rust/Frontend 测试通过。
- debug app build 通过。
- UI 截图验收通过，确认没有大幅 UI 改造。
- 文档更新说明 clipboard 和 Apple Speech 的真实能力边界。

## 18. 暂不做

以下能力不进本轮：

- Android。
- 远程手机麦克风。
- Less Computer / coding agent。
- 大型 marketplace。
- 复杂聊天式 Ask UI。
- 大幅 Settings redesign。
