# OpenTypeless x OpenLess x 闪电说 x Typeless 差异 Spec

Date: 2026-07-08
Target repo: `/Users/bytedance/个人项目/opentypeless`
OpenTypeless HEAD inspected: `a83456f` (`Update README visual tour`)
Reference repo: `/Users/bytedance/个人项目/openless`
OpenLess HEAD inspected: `7de0c04` (`docs: 用协调沉降定律重构 README 能力/增强叙事,凸显开屏即授权完基础设施 (#765)`)

## 1. 人话结论

现在的 OpenTypeless 已经不是早期那个“有语音输入壳子，但底层不够稳”的状态了。对 OpenLess 的一批核心借鉴已经落地：

- API key 已有系统凭据库迁移和读取路径。
- 听写 hotkey 默认已对齐 Typeless: macOS `Fn`，Windows `RightAlt`。
- Ask hotkey 已独立出来: macOS `Fn+Space`，Windows `RightAlt+Space`。
- 翻译热键也有默认角色: macOS `Fn+LeftShift`，Windows `RightAlt+LeftShift`。
- 插入层已有策略化结构: auto / keyboard / clipboard paste / clipboard copy only / Windows SendInput。
- 剪贴板粘贴后已有 text-only restore。
- 流式插入 worker 已有，但默认关闭。
- Ask Anything 已从大面板改成轻量便签式浮窗，支持失焦关闭。
- Scenes 已有本地自定义、内置模板、导入导出和激活逻辑。
- Apple Speech 和 custom Whisper 已作为本地/自托管 STT 路径存在。
- Settings 已被收敛，General 只露出核心项，Advanced 折叠。
- 默认开机启动已经是 `true`。

但如果拿现在的 OpenTypeless 对比 OpenLess、闪电说、Typeless，还剩下几个真正影响体验的差异：

1. **胶囊状态体验还没有 OpenLess 丝滑。**
   当前有 preparing / recording / transcribing / polishing / outputting，但 OpenLess 的胶囊更像一个连续的“语音光效状态机”：预热、入场、离场、thinking orb、音量驱动、退出动画都更细。

2. **Ask 还没有完全变成 Typeless/闪电说那种上下文命令入口。**
   当前 Ask 是“语音问一句，弹一个答案”，已经能带选中文本上下文。但 Typeless 的重点是“选中文本后直接用语音编辑/解释/总结/翻译”，闪电说的重点是“结合屏幕上下文、记忆、知识库、技能帮你回复”。OpenTypeless 还没有到那层。

3. **本地 ASR 没有 OpenLess 的模型管理能力。**
   OpenTypeless 有 Apple Speech 和 custom Whisper endpoint；OpenLess 有 Qwen3-ASR、Windows Foundry Local Whisper、Sherpa ONNX、模型下载/删除/设为默认等管理 UI。这里差距还比较明显。

4. **Scenes 还不是 OpenLess 的 Style Pack 系统。**
   OpenTypeless 的 Scenes 是“本地 prompt 模板”；OpenLess 的 Style Pack 是“风格包系统”：元数据、示例、启停、激活、ZIP 导入导出、marketplace、切换热键。我们已经有骨架，但还不完整。

5. **个性化和学习能力还弱于 Typeless/闪电说。**
   Typeless 强调个人风格、个人词典、按 app 调整语气；闪电说强调沟通记忆、知识库、常用回复。OpenTypeless 现在主要是手动 dictionary + custom prompt + scenes，没有自动学习闭环。

6. **输出质量缺少可回归的评测体系。**
   prompt 规则已经不少，但还没有一套固定样例去测“去口癖、去重复、自我修正、列表格式、跨语言、专有名词、选中文本命令”的质量。

7. **设置 UI 已经在变简洁，但功能解释仍要克制。**
   不能为了补齐差异把前端变成 OpenLess 那种重设置中心，也不能复制闪电说的沟通 Agent 大概念。OpenTypeless 应继续保持高级、轻、极简。

推荐补齐顺序：

1. P0: 胶囊状态流畅度和稳定验收。
2. P0: Ask selected-text command workflow 补完整。
3. P1: 输出质量评测集和 prompt 回归。
4. P1: Scenes 升级为轻量 Style Profiles，不先做 marketplace。
5. P1: 本地 ASR 模型管理 MVP。
6. P2: 自动 dictionary / correction learning。
7. P2: 闪电说式知识库/常用回复/沟通记忆，先做轻量本地版，不做大 agent。
8. P3: Android、远程手机麦克风、完整 marketplace、技能执行 agent，暂不进入当前阶段。

## 2. 信息来源和可信度

### 2.1 本地代码来源

OpenTypeless 当前 worktree 是 dirty 的，本 spec 以“当前本地文件”为准，不只看 git HEAD。

重点检查文件：

- `src-tauri/src/storage/mod.rs`
- `src-tauri/src/hotkey.rs`
- `src-tauri/src/native_hotkey.rs`
- `src-tauri/src/pipeline.rs`
- `src-tauri/src/output/*`
- `src-tauri/src/credentials.rs`
- `src-tauri/src/selection.rs`
- `src-tauri/src/commands/ask.rs`
- `src-tauri/src/stt/*`
- `src/components/Capsule/*`
- `src/components/AskPanel/AskPanel.tsx`
- `src/components/Settings/*`
- `src/lib/scenes/*`
- `src/stores/appStore.ts`

OpenLess 重点检查文件：

- `openless-all/app/src/components/Capsule.tsx`
- `openless-all/app/src/pages/QaPanel.tsx`
- `openless-all/app/src/pages/settings/LocalModelSection.tsx`
- `openless-all/app/src-tauri/src/hotkey.rs`
- `openless-all/app/src-tauri/src/commands/hotkeys.rs`
- `openless-all/app/src-tauri/src/insertion.rs`
- `openless-all/app/src-tauri/src/coordinator/polish_flow.rs`
- `openless-all/app/src-tauri/src/persistence/style_pack.rs`
- `openless-all/app/src-tauri/src/commands/style_packs.rs`
- `openless-all/app/src-tauri/src/asr/local/*`
- `openless-all/app/src-tauri/src/types.rs`

### 2.2 公开竞品来源

Typeless:

- Homepage: https://www.typeless.com/
- First dictation quickstart: https://www.typeless.com/help/quickstart/first-dictation
- Installation/setup: https://www.typeless.com/help/installation-and-setup
- Voice Superpowers release note: https://www.typeless.com/help/release-notes/macos/voice-superpowers

公开页面明确写到：

- Typeless 桌面默认听写快捷键是 macOS `Fn`、Windows `Right Alt`。
- 它强调“说自然语言，输出像认真打出来的文字”。
- 它支持跨 app、100+ languages、翻译、个人风格、个人词典、按 app 适配语气。
- Voice Superpowers 是选中文本后按 Typeless hotkey 说命令，支持改写、翻译、总结、解释等。

闪电说:

- Official site: https://shandianshuo.cn/
- Legacy Daiti site: https://daiti.ai/
- Public introduction issue: https://github.com/ruanyf/weekly/issues/8287

公开页面明确写到：

- 闪电说定位是“沟通 Agent”，不只是语音输入。
- 它有“直接说”和“帮我说”两条心智。
- 它强调屏幕上下文、沟通记忆、知识库、常用回复、技能执行。
- 它支持 Mac / Windows，Android 内测，iOS / Linux 标为即将发布。

OpenLess:

- GitHub: https://github.com/Open-Less/openless
- Local clone: `/Users/bytedance/个人项目/openless`

## 3. 产品定位差异

| 产品 | 一句话定位 | OpenTypeless 当前差异 |
| --- | --- | --- |
| OpenLess | 开源、本地优先、把语音稳定变成当前光标里的可用文字 | 我们已补了很多底层工程，但本地 ASR、胶囊体验、Style Pack 还弱 |
| Typeless | 闭源商业化、全设备、让语音写作像认真打字 | 我们已对齐默认 hotkey 和部分 Ask/选中文本能力，但个性化、自动学习、移动端弱 |
| 闪电说 | 沟通 Agent，帮你在具体对象/场景里把话说好 | 我们目前还是语音输入 + Ask 工具，不是沟通记忆/知识库/技能 agent |
| OpenTypeless | 开源跨平台、BYOK + Cloud、语音输入/润色/Ask/选中文本 | 优势是开放、Linux、provider 选择、云端账号/订阅；短板是体验闭环和个性化 |

关键判断：

OpenTypeless 不应该复制任一产品。正确方向是：

- 借 OpenLess 的底层稳定性。
- 借 Typeless 的“少设置、会写、会改、按 app 适配”的体验目标。
- 借闪电说的“上下文、记忆、知识库”思路，但只做轻量版，避免变成重 agent。
- 保留 OpenTypeless 的开源、BYOK、跨平台、极简 UI。

## 4. 已经基本追平的部分

### 4.1 Hotkey 默认值

当前 OpenTypeless:

- macOS 听写: `Fn`
- Windows 听写: `RightAlt`
- Linux 听写: `Ctrl+/`
- macOS Ask: `Fn+Space`
- Windows Ask: `RightAlt+Space`
- Linux Ask: `Ctrl+.`
- macOS 翻译: `Fn+LeftShift`
- Windows 翻译: `RightAlt+LeftShift`

结论：

- 和 Typeless 的桌面听写默认值已经对齐。
- 和 OpenLess 的默认值不同。OpenLess local code 默认 macOS `RightOption`，Windows `RightControl`，这不是我们要追的目标。
- Ask 用独立 hotkey 是合理的。它避免把“问问题”和“插入文字”混在一起，对 OpenTypeless 当前阶段更安全。

还缺什么：

- UI 只露出了听写和 Ask，翻译/编辑/切换场景这些角色还没有完整用户体验。
- 右侧/左侧修饰键区分没有 OpenLess 那么完整。
- MediaPlayPause / 耳机线控触发没有做。

建议：

- 保留当前默认 hotkey，不改。
- P1 再做“高级快捷键”折叠区，只露出翻译、编辑选区、切换 Scene，不要做大矩阵。
- 不做 MediaPlayPause，除非用户明确要耳机线控。

验收：

- 新装 macOS 默认 `Fn`。
- 新装 Windows 默认 `RightAlt`。
- Ask 默认独立可用。
- 禁用 Ask 后重启仍保持禁用。
- hotkey 冲突时有短提示，不出现复杂错误面板。

### 4.2 凭据安全

当前 OpenTypeless:

- `SystemCredentialVault` 基于 OS credential vault。
- legacy config key 可迁移到 vault。
- STT/LLM 运行时从 vault resolve secret。
- 迁移失败时保留原 plaintext key，不误删。

结论：

- OpenLess 的“不要把 API key 放 settings.json”方向已经借到位。

还缺什么：

- Linux Secret Service 不可用时的 UI 提示需要更明确。
- Credential status UI 还可以更“人话”，比如显示“已保存到系统钥匙串”，而不是让用户以为 key 消失了。

建议：

- P1 做一行 credential health summary。
- 不做独立 Credential Manager 页面，太重。

验收：

- 保存 key 后 config 文件不包含明文 key。
- 切 provider 不丢 key。
- 删除 key 后 provider test 明确提示缺 key。
- Linux vault 不可用时不静默降级 plaintext。

### 4.3 插入层可靠性

当前 OpenTypeless:

- 有 insertion strategy。
- 有 keyboard、clipboard paste、clipboard copy only、Windows SendInput。
- 有 text-only clipboard restore。
- 有 streaming chunk output 路径。
- 有 output result 记录到 history 的结构。

结论：

- OpenLess 的核心插入策略已经大部分吸收。

还缺什么：

- macOS 输入法/secure input/特殊 app 的细分诊断不如 OpenLess。
- Linux 没有 fcitx commit_text 这种更深路径。
- Windows TSF 输入法路径没有 OpenLess 那么完整。
- streaming insert 默认关闭，用户体感默认仍是“等完整结果”。

建议：

- P0/P1 先把 streaming insert 做成可控实验: 只在安全策略下开，不默认强开。
- P1 增加“插入失败后怎么处理”的极简 toast 文案。
- P2 再研究 Linux fcitx / Windows TSF，别先把 UI 搞复杂。

验收：

- keyboard 失败会 fallback，不丢文本。
- clipboard paste 后能恢复原文本剪贴板。
- 非文本剪贴板不承诺完整恢复，文案必须写 text-only。
- streaming insert 中途失败时 full output 可追回到 clipboard。
- 目标 app 变化时不继续往新 app 乱打。

### 4.4 Ask 便签 UI

当前 OpenTypeless:

- Ask 独立窗口是无 decorations、transparent、shadow false。
- 前端是 floating note，而不是全屏设置面板。
- 失焦关闭，Esc 关闭，点击空白关闭。
- recording 时关闭会 abort。
- processing 时关闭会忽略后续结果，不会突然弹回。

结论：

- 用户之前说“大阴影、像面板、不像便签”的问题已经被修过方向。

还缺什么：

- Ask 仍是一次性问答，不是 Typeless 的完整选中文本编辑入口。
- 没有多轮追问。
- 没有 pin。
- 没有输入文字 fallback。
- 没有明显地承载“解释选中文本、总结选中文本、翻译选中文本、改写选中文本”的分流。

建议：

- P0 补 selected-text command workflow，而不是先做多轮聊天。
- 不要把 Ask 做成 OpenLess QaPanel 那种聊天卡片，当前用户明确想要便签感。
- 后续如果做多轮，必须是折叠/轻量，不改变默认形态。

验收：

- 未选中文本: Ask 回答问题，不插入当前 app。
- 选中文本 + “解释/总结/这段什么意思”: 弹 Ask，绝不替换原文。
- 选中文本 + “改写/润色/变短/翻译”: 替换原文或 copy fallback。
- 选中文本 + 模糊指令: 默认弹 Ask，不做破坏性替换。

## 5. 还需要重点补齐的差异

### P0 Feature 1: 胶囊状态体验继续向 OpenLess 靠拢

#### 参考行为

OpenLess 的胶囊不是简单显示文字状态，而是一个连续动画系统：

- idle 时基本消失。
- recording 进入时有短入场动画。
- 麦克风未就绪时有 warming/preparing 态。
- 音量驱动 waveform。
- transcribing/polishing 时波形过渡成 orb / thinking dots。
- done/cancelled/error 有收尾动画。
- 有 warmupMs 学习，用上次麦克风 ready 耗时预测动画节奏。
- 退出不是立即 unmount，而是保留最后一帧播放离场。

#### 当前 OpenTypeless

有这些状态：

- idle
- preparing
- recording
- transcribing
- polishing
- outputting
- error

也有：

- framer-motion layout 动画。
- 状态组件拆分。
- preparing/transcribing/polishing 可取消。
- 部分 transcript 可显示。

差距：

- 状态之间仍像“切组件”，不是连续形变。
- preparing -> recording、recording -> transcribing、polishing -> outputting 的动画桥不够自然。
- outputting 太短，用户可能看不到完成反馈。
- 没有学习 warmup 时长。
- 没有明确 cancelled 完成态。
- 视觉高级感比之前好，但还不是 OpenLess 那种“轻、跟手、有生命”的状态。

#### 推荐方案

保持 OpenTypeless 极简胶囊，不复制 OpenLess 的 SiriGL 大光效。做“轻量连续状态机”：

1. 新增 capsule visible lifecycle:
   - hidden
   - entering
   - active
   - leaving

2. 新增 lastVisibleState:
   - pipeline 进入 idle 后，不立即缩回 36px。
   - 保留 450-600ms 做完成/离场动画。

3. 新增 warmupMs:
   - 记录 start -> recording 的耗时。
   - 用 localStorage 保存 EMA。
   - preparing 动画时长根据 warmupMs 调整。

4. 统一状态视觉:
   - recording: 音量波形为主。
   - transcribing: 小光点 + partial transcript。
   - polishing/thinking: 更安静的三点或细线动画。
   - outputting: 短 check 或收束动画。
   - error: 红点 + 短文案。

5. 不增加大阴影、不加彩色背景、不做全屏光效。

#### UI 限制

- 胶囊仍是小尺寸，不改主风格。
- 不用 OpenLess 的大 canvas 光效。
- 不加解释文案。
- 不把胶囊变成工具栏。

#### 文件范围

- `src/components/Capsule/index.tsx`
- `src/components/Capsule/CapsulePreparing.tsx`
- `src/components/Capsule/CapsuleRecording.tsx`
- `src/components/Capsule/CapsuleProcessing.tsx`
- `src/components/Capsule/CapsulePolishing.tsx`
- `src/components/Capsule/CapsuleComplete.tsx`
- `src/hooks/useCapsuleResize.ts`
- `src/hooks/useTauriEvents.ts`
- `src/stores/appStore.ts`

#### 验收标准

- start 后 100-200ms 内有反馈，不出现“按了没反应”的空窗。
- recording 到 transcribing 不跳宽度。
- polishing 到 outputting 有 300-600ms 的完成反馈。
- pipeline idle 后胶囊优雅收回，不突然消失。
- reduced motion 下动画简化但状态仍清楚。
- 截图和录屏验收：白底、浅色、深色都不出现大阴影块。

### P0 Feature 2: Ask selected-text workflow 补完整

#### 参考行为

Typeless:

- 选中文字。
- 按 Typeless hotkey。
- 说“rewrite this as professional email / summarize / explain / translate”。
- 对编辑类命令直接替换选区。
- 对阅读类命令弹回答，不破坏原文。

OpenLess:

- 有 SelectionAsk / QaPanel。
- QA 是单独浮窗，支持语音和文字。
- 可以多轮问答。

闪电说:

- 长按“帮我说”会结合屏幕上下文和记忆生成回复。
- 知识库和常用回复可参与回答。

#### 当前 OpenTypeless

已有：

- selected text capture。
- route_selected_text_command。
- Ask selected text context。
- Ask 结果 metadata: usedSelectedText、selectedTextTruncated、intent。
- Ask popup。

差距：

- 用户还不一定知道“选中文本后可以问/改”。
- 听写 pipeline 和 Ask pipeline 之间的命令心智不够统一。
- 不支持文字输入追问。
- 不支持多轮。
- 没有“选中文本 command 的验收用例集”。

#### 推荐方案

先补“命令闭环”，不要先做聊天。

1. 明确三类 intent:
   - Insert: 没有选中文本，普通听写插入。
   - ReplaceSelection: 有选中文本，用户说改写/翻译/缩短/扩写/纠错。
   - PopupAnswer: 有选中文本，用户说解释/总结/问问题。

2. UI 只做极简提示:
   - General 里 Ask 卡片文案改成“Ask about selected text or a quick question”。
   - 不新增复杂教程。

3. Ask 便签结果显示上下文标签:
   - Selected text
   - Selected text truncated
   - Search opened

4. 后端必须默认安全:
   - 模糊命令不替换。
   - 无 LLM 时不把口头指令插回选区。
   - selected text 注入 prompt 时标记为 untrusted。

#### 文件范围

- `src-tauri/src/selection.rs`
- `src-tauri/src/pipeline.rs`
- `src-tauri/src/commands/ask.rs`
- `src/components/AskPanel/AskPanel.tsx`
- `src/components/Settings/GeneralPane.tsx`
- `src/lib/__tests__/tauri-ask.test.ts`

#### 验收标准

- 选中一段文本，说“总结这段”，弹 Ask，不替换原文。
- 选中一段文本，说“翻译成英文”，替换选区。
- 选中一段文本，说“这是什么意思”，弹 Ask。
- 选中一段文本，说“帮我看看”，默认 Ask，不替换。
- 没选中文本时普通听写仍插入。
- 失败时结果可复制，不丢。

### P1 Feature 3: 输出质量评测集

#### 参考行为

Typeless 的核心卖点不是“能转写”，而是：

- 去口癖。
- 去重复。
- 自我修正。
- 自动格式化列表/步骤。
- 语气适配。
- 专有名词准确。
- 翻译像本地人写。

闪电说也强调：

- 结构化整理。
- 去重复和啰嗦。
- 修正错字。
- 专属词典认准人名和术语。

#### 当前 OpenTypeless

prompt 里已经覆盖不少规则，但没有固定 benchmark。

#### 推荐方案

新增 `docs/fixtures` 或 `src-tauri/tests/fixtures` 的质量样例集。

第一批样例：

1. filler removal:
   - raw: “um I think we should like ship this tomorrow”
   - expected traits: 无 um/like，意思不变。

2. self correction:
   - raw: “meet at 7 no actually 3 pm”
   - expected: 只保留 3 PM。

3. list formatting:
   - raw: “shopping list bananas oat milk chocolate”
   - expected: bullets。

4. Chinese casual cleanup:
   - raw: “就是那个我们明天先看一下这个数据然后再决定”
   - expected: 简洁中文，不乱加事实。

5. selected text rewrite:
   - input selected text + instruction。
   - expected: replacement only。

6. selected text ask:
   - selected text + “解释一下”。
   - expected: popup answer。

7. dictionary:
   - term: OpenTypeless / Qwen / 飞书。
   - expected: 不被错改。

8. translation:
   - source language + target language。
   - expected: 输出目标语言，保留专名。

#### UI 限制

不做可视化评测面板。先做测试和文档。

#### 验收标准

- 每次改 prompt 或 LLM payload 时跑 fixture。
- 测试不要求逐字完全相同，但要求 trait matcher 通过。
- history 记录 active_scene/prompt chars，不存过长 prompt。

### P1 Feature 4: Scenes 升级为轻量 Style Profiles

#### 参考行为

OpenLess Style Packs:

- Built-in + imported。
- prompt、tags、examples、author、version。
- enable/disable。
- active style。
- ZIP import/export。
- marketplace install/publish。
- switch style hotkey。

当前 OpenTypeless Scenes:

- 7 个 built-in scene。
- 自定义 scene。
- active scene。
- import/export JSON。
- duplicate / edit / delete / activate。

差距：

- 没有 examples。
- 没有 tags/version/author。
- 没有 style validation diagnostics。
- 没有 switch scene hotkey UI。
- 没有 marketplace。

#### 推荐方案

不要直接做 marketplace。先把 Scenes 改成“本地 Style Profiles”能力增强：

1. 数据结构扩展:
   - tags?: string[]
   - examples?: { input: string; output: string }[]
   - version?: string
   - author?: string

2. UI 仍保持简单:
   - 默认列表只显示 name/description/active。
   - 详情里再显示 prompt/examples。
   - 不新增大卡片海。

3. 导入导出兼容:
   - 继续支持旧 JSON。
   - 新 JSON 版本化。
   - 大字段限长。

4. switch scene hotkey:
   - 后端角色已有 `switch_scene`。
   - P1 可以在 Advanced 里露一个折叠项。
   - 行为：循环 enabled scenes 或打开极简 quick picker，二选一。
   - 第一版建议“循环 active scene”，因为 UI 最少。

#### 验收标准

- 旧 scenes JSON 可导入。
- 新 style profile JSON 可导出导入。
- active scene 在 history 里有记录。
- prompt 太长会截断并提示。
- switch scene 不打断当前录音。

### P1 Feature 5: 本地 ASR 模型管理 MVP

#### 参考行为

OpenLess:

- Qwen3-ASR。
- Windows Foundry Local Whisper。
- Sherpa ONNX。
- 模型下载/删除/设为默认。
- 平台能力检测。

当前 OpenTypeless:

- Apple Speech provider。
- custom Whisper endpoint。
- 没有内置模型下载/运行管理。

差距：

- 对普通用户来说，“custom Whisper”不是本地 ASR，是“你自己搭一个服务”。
- 闪电说强调端侧优先、本地模型、低延迟；OpenTypeless 现在还不能这么强地讲。

#### 推荐方案

分两阶段：

Phase 1: 本地 ASR 设置解释清楚

- 在 STT 里把 `Apple Speech` 和 `Local / Custom Whisper` 分清。
- custom Whisper 文案说清楚“需要你本地跑一个 Whisper-compatible server”。
- 加一键测试。

Phase 2: 模型管理 MVP

- 只选一个跨平台路径先做，建议 `faster-whisper server helper` 或 `whisper.cpp/sherpa`。
- UI 只做:
  - local engine status
  - model downloaded / missing
  - download / delete
  - test
- 不做多模型市场。

#### 验收标准

- macOS Apple Speech 不需要 API key。
- custom Whisper endpoint 配错时错误可读。
- 本地模型下载失败可恢复。
- 模型管理不会把 Settings 变复杂，必须折叠在 Speech Recognition 的 Local section。

### P1 Feature 6: 个性化/按 app 风格

#### 参考行为

Typeless:

- personalized style and tone。
- personal dictionary。
- different tones for each app。

闪电说:

- 每个人/对方有沟通记忆。
- 回复时加载偏好、习惯、关键事项、历史上下文。

当前 OpenTypeless:

- app_detector 有 app type。
- prompt 有 app type add-ons。
- dictionary 手动添加。
- scenes 手动选择。

差距：

- 没有 per-app rule。
- 没有自动风格学习。
- 没有联系人/对象记忆。
- 没有 correction learning。

#### 推荐方案

先做轻量、可解释、可关的本地个性化：

1. Per-app style rules:
   - app type: email/chat/code/document/general 已有。
   - 用户可选择某个 app type 默认 scene。
   - 不做复杂 per bundle UI，先用 app type。

2. Dictionary suggestions:
   - 从用户手动修改/重复失败词中建议添加。
   - 第一版可先不自动采集编辑行为，只从 history 中抽取高频 unknown capitalized terms。

3. Correction rules:
   - 类似 OpenLess correction store。
   - 用户手动添加 “把 X 纠正为 Y”。
   - 应用在 STT 后、LLM 前。

#### UI 限制

- 不新增“Personalization”大页面。
- 放在 Dictionary 里增加一个极简 `Corrections` tab 或折叠 section。
- Per-app 默认 scene 放到 Scenes 详情/Advanced。

#### 验收标准

- 用户可添加 correction rule。
- correction 不作用于 selected text 的 untrusted context 指令。
- app type 默认 scene 不影响手动 active scene 的优先级。
- 所有个性化默认本地存储。

### P2 Feature 7: 闪电说式知识库/常用回复

#### 参考行为

闪电说:

- 知识库包括风格、词典、常用回复、产品资料。
- 回复时自动参考知识库。
- 技能可请求数据、执行代码、调用工具，再生成回复。

当前 OpenTypeless:

- dictionary。
- scenes。
- Ask。
- 无知识库。
- 无常用回复。
- 无工具执行。

差距：

- OpenTypeless 不是沟通 Agent。
- 如果用户要“帮我回复这个客户”，当前只能靠 prompt 和选中文本，不能加载对方偏好/产品资料。

#### 推荐方案

不要直接做 agent。先做一个轻量 Local Knowledge：

1. `Knowledge Snippets`
   - 标题。
   - 内容。
   - tags。
   - enabled。

2. 使用场景:
   - Ask 可以选择性使用。
   - Scenes 可以引用 enabled snippets。
   - 默认不自动塞所有知识，避免 prompt 膨胀。

3. 常用回复:
   - 可以作为 snippets 的一种 type。
   - 只在用户手动选择 scene 或 Ask intent 时使用。

4. 不做技能执行:
   - 不执行代码。
   - 不请求外部 API。
   - 不连接数据库。

#### UI 限制

- 不做“知识库中心”大页面。
- 可以先放 Dictionary 或 Scenes 下的一个折叠区。
- 不要把用户带进复杂 RAG 设置。

#### 验收标准

- 用户可新增一条 knowledge snippet。
- Ask/Scene 可明确使用它。
- prompt 显示截断和来源。
- 默认不上传未启用知识。

## 6. 暂不建议补的差异

### 6.1 完整 Marketplace

OpenLess 有 Style Pack Marketplace。OpenTypeless 暂不建议做。

原因：

- 需要后端、审核、账号、发布、滥用治理。
- 当前更重要的是本地 style profile 体验。
- 做早了会把 UI 和运营都搞重。

结论：

- P3 以后再看。
- 现在只做导入导出和内置模板增强。

### 6.2 闪电说技能执行 Agent

闪电说可以“请求数据、执行代码、调用工具”。OpenTypeless 暂不建议做。

原因：

- 安全风险高。
- 产品会从语音输入变成 agent 平台。
- 用户当前更在意胶囊、Ask、hotkey、输入稳定性。

结论：

- 不做自动执行。
- 可以做“Ask 搜索/打开网页”这类低风险动作，但也要可关闭。

### 6.3 Android / iOS

Typeless 有 iOS/Android，闪电说 Android 内测，OpenLess 有 Android 方向。

OpenTypeless 暂不建议立刻追。

原因：

- 当前桌面体验还没完全打磨。
- 移动端输入法是另一套权限/分发/输入法架构。
- 会显著分散工程精力。

结论：

- P3 以后再规划。
- 当前坚持桌面 macOS/Windows/Linux。

### 6.4 远程手机麦克风

OpenLess 有 remote input/server 方向。

OpenTypeless 暂不建议做。

原因：

- 网络、配对、安全、延迟、局域网权限都复杂。
- 不是当前最大缺口。

结论：

- 暂不做。

### 6.5 OpenLess 大光效 Capsule

OpenLess 胶囊很有表现力，但 OpenTypeless 不应该直接复制。

原因：

- 用户明确要求 UI 极简、高级、less is more。
- OpenTypeless 当前整体视觉更克制。
- 大 canvas 光效可能带来性能和审美风险。

结论：

- 学习状态连续性，不复制视觉风格。

## 7. 总体优先级矩阵

| Priority | Feature | 为什么重要 | UI 风险 | 建议 |
| --- | --- | --- | --- | --- |
| P0 | 胶囊状态流畅度 | 用户每天都看，影响“有没有学到 OpenLess” | 中 | 只改胶囊，不动主 UI |
| P0 | Ask selected-text command workflow | 对齐 Typeless 核心用法 | 中 | 保持便签，不做聊天 |
| P1 | 输出质量评测集 | 防止 prompt 回归 | 低 | 主要测试/fixtures |
| P1 | Scenes -> Style Profiles | 补 OpenLess Style Pack 的本地价值 | 中 | 不做 marketplace |
| P1 | 本地 ASR 模型管理 MVP | 补闪电说/OpenLess 本地优先差距 | 中高 | 折叠到 STT Local |
| P1 | Per-app style / correction rules | 对齐 Typeless 个性化 | 中 | 放在高级/折叠 |
| P2 | Knowledge snippets / 常用回复 | 借闪电说知识库思路 | 高 | 只做本地轻量版 |
| P2 | 多轮 Ask / pin / text fallback | 对齐 OpenLess QA | 中高 | 默认仍是一张便签 |
| P3 | Marketplace | 运营和后端重 | 高 | 暂缓 |
| P3 | Android/iOS | 另一套产品 | 高 | 暂缓 |
| P3 | 技能执行 agent | 安全风险高 | 高 | 暂缓 |

## 8. 推荐实施计划

### Sprint 1: 胶囊体验和 Ask 命令闭环

目标：

- 让用户肉眼感觉“流程顺了”。
- Ask 选中文本不再像半成品。

任务：

1. 胶囊 lifecycle:
   - entering/leaving。
   - lastVisibleState。
   - outputting 完成态停留。
   - warmupMs EMA。

2. 胶囊截图/录屏验收:
   - idle。
   - preparing。
   - recording。
   - transcribing。
   - polishing。
   - outputting。
   - error。

3. Ask selected-text:
   - 明确 PopupAnswer / ReplaceSelection。
   - 模糊命令不替换。
   - Ask UI 显示 selected text 标签。

4. 测试:
   - Vitest: AskPanel、Capsule flow。
   - Rust tests: selected text router、ask body、pipeline policy。

验收：

- 不改 Settings 主结构。
- 不增加大块说明。
- 不出现大阴影/巨大空白。

### Sprint 2: 质量评测和 Scenes 增强

目标：

- 把“说得像打出来的”变成可测。
- 把 Scenes 从 prompt list 升级到轻量 style profile。

任务：

1. 添加 quality fixtures。
2. 添加 style profile metadata。
3. import/export 兼容旧版本。
4. switch scene role 最小闭环。

验收：

- 旧 scenes 不坏。
- UI 不复杂。
- 质量样例可自动跑。

### Sprint 3: Local ASR MVP

目标：

- 让“本地优先”不只是 custom endpoint。

任务：

1. STT UI 文案区分 Apple Speech / local server / cloud。
2. 选择一个模型管理路径。
3. 模型下载/删除/test。
4. 失败诊断。

验收：

- 不配 API key 也能用一个本地路径。
- 模型下载失败不污染设置。
- Local section 折叠，不影响普通用户。

### Sprint 4: 轻量个性化

目标：

- 对齐 Typeless 的 personal dictionary / app-specific tone，借闪电说知识库但不变重。

任务：

1. Correction rules。
2. App type -> default scene。
3. Knowledge snippets MVP。

验收：

- 默认关闭/本地。
- prompt 可解释。
- 用户能删干净。

## 9. UI Guardrails

所有后续实现必须遵守：

1. 不重做主导航。
2. 不把 Settings 变成大控制台。
3. General 继续保持少项，只放用户每天需要理解的东西。
4. Advanced 默认折叠。
5. Ask 继续是便签，不是聊天 app。
6. 胶囊可以更丝滑，但不能变大、变花、变重。
7. 每个 UI 变更都要截图验收。
8. 文字少一点，状态清楚一点。
9. 所有高级能力先藏起来，默认路径只保留:
   - 听写快捷键。
   - Ask 快捷键。
   - 输出方式。
   - provider。
   - 必要权限。

## 10. 当前差异清单

### 10.1 和 OpenLess 的差异

已接近：

- OS credential vault。
- hotkey role model。
- 插入策略。
- 剪贴板恢复。
- Windows SendInput。
- streaming insert infrastructure。
- local scenes。
- Ask popup。
- selected text router。

仍有差距：

- 胶囊动画连续性。
- 本地 ASR 模型管理。
- Style Pack 完整性。
- Marketplace。
- QA 多轮文字+语音面板。
- Linux fcitx commit path。
- Windows TSF/IME 更深集成。
- Remote input。
- Less Computer/coding agent。

不建议追：

- Marketplace 后端。
- Android。
- Remote input。
- Less Computer。

### 10.2 和 Typeless 的差异

已接近：

- 默认听写 hotkey。
- 跨 app 语音输入。
- 语音润色。
- 翻译基础。
- 个人词典基础。
- Ask 基础。
- 选中文本基础。
- 本地历史。

仍有差距：

- 个性化风格学习。
- 自动 dictionary 建议。
- 按 app 语气。
- 选中文本 voice superpowers 的自然度。
- Ask quick actions/search/services。
- 质量评测和 polish consistency。
- 移动端。
- trust/security/compliance 包装。

不建议立刻追：

- 移动端。
- 团队管理。
- 合规认证包装。

### 10.3 和闪电说的差异

已接近：

- 直接说: 语音整理后落入输入框。
- 专有词 dictionary 基础。
- 去重复/结构化由 prompt 支持。
- Mac/Windows 桌面能力。

仍有差距：

- 帮我说: 长按结合上下文生成回复。
- 沟通记忆。
- 知识库。
- 常用回复。
- 技能执行。
- 屏幕上下文理解。
- 中文场景表达/聊天软件心智。

不建议立刻追：

- 技能执行。
- 自动请求数据/执行代码。
- 复杂对象记忆。

可以轻量借鉴：

- 直接说/帮我说这两个心智可以映射成:
  - Dictation = 直接说。
  - Ask selected text/context = 帮我说的轻量版。
- Knowledge snippets 可以作为后续本地能力。

## 11. 成功标准

这轮补齐完成后，应该达到：

1. 用户按热键后，胶囊反馈明显、稳定、不卡。
2. 录音结束后，transcribing/thinking/outputting 状态变化顺滑。
3. Ask 看起来像一张临时便签，不像复杂面板。
4. 选中文本后，说“解释/总结/翻译/改写”结果符合预期。
5. 设置页仍然清爽，不需要用户理解一堆底层名词。
6. 本地/隐私路线说得更实，不再只靠 custom endpoint。
7. prompt 输出质量有回归测试，不靠感觉。
8. OpenTypeless 保留自己的高级感和开源定位，不变成 OpenLess/Typeless/闪电说的混合怪。

## 12. 下一步建议

建议先开一个实施 spec：

`2026-07-08-capsule-and-ask-command-implementation-spec.md`

范围只包含：

1. 胶囊 lifecycle 和动画平滑。
2. Ask selected-text command workflow。
3. 对应验收截图和测试。

先别把本地 ASR、Style Profile、知识库一起塞进来。那样会把前端和心智都搞复杂。
