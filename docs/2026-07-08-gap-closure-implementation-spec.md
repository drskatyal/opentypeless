# OpenTypeless 本期差异补齐 Spec

Date: 2026-07-08
Repo: `/Users/bytedance/个人项目/opentypeless`
Status: reviewed; Slice A implemented and verified; Slice B Ask hotkey recording lifecycle implemented and verified

## 1. 人话结论

本期不是把 OpenTypeless 改成 OpenLess、Typeless 或闪电说。

本期要做的是：把已经讨论确认、对日常体验最有价值、且不会让 UI 变复杂的差异补齐。

最终产品范围仍然只包含 5 件事：

1. 胶囊状态做轻量质感优化。
2. Ask Anything 做成极简便签，支持选中文本上下文，结果只保留 Copy。
3. AI polish 增加 4 个基础输出风格：Minimal / Clean / Structured / Professional。
4. Dictionary 保留手动词典，并增加简单纠错规则：Wrong phrase -> Correct phrase。
5. Streaming insertion 不默认开启，只补稳定性和回退验收。

明确不做：

- 不做完整 OpenLess Style Pack Marketplace。
- 不做闪电说式知识库、常用回复、沟通 Agent。
- 不做截图上下文本期落地。
- 不做本地 ASR 模型下载/删除/管理 UI。
- 不大改 Settings。
- 不把 General 变复杂。
- 不给 Ask 增加 Replace / Insert 等会破坏原文的动作。

实施上分两步，避免一次改动过大：

1. **Implementation Slice A（先做）**：Polish Style 四模式 + Dictionary Corrections。这两项是数据模型和 prompt 基础，最适合先落地并加测试。
2. **Implementation Slice B（随后）**：Ask 便签验收收口 + 胶囊轻量质感 + Streaming 回退验收。这三项更偏 UI/端到端，需要在 A 稳定后验收。

一句话：

> 本期只补“用户每天会感受到的核心链路”：听写更顺、Ask 更像 Typeless、polish 风格更好懂、词典更实用。

## 2. 背景

过去几轮已经对比过：

- OpenLess: 更强的胶囊状态、Style Pack、Streaming insertion、本地 ASR、插入稳定性。
- Typeless: 更明确的默认 hotkey、Ask anything、选中文本语音处理、个人词典、个人风格。
- 闪电说: 更偏沟通 Agent、知识库、常用回复、屏幕/沟通上下文。

当前 OpenTypeless 已经补齐了不少底层基础：

- macOS 听写默认 `Fn`。
- Windows 听写默认 `RightAlt`。
- Ask 有独立 hotkey。
- Ask 已经有便签式浮窗基础。
- Settings 已做过简化。
- 启动项默认开启。
- 插入策略、剪贴板回退、history 诊断、scene/custom prompt 都有基础。

所以本期不再做大面积重构，而是把几个关键差异打磨成稳定可用的产品体验。

## 3. 产品原则

### 3.1 UI 原则

本期所有 UI 改动必须遵守：

- Less is more。
- 不新增复杂设置矩阵。
- 不做大卡片堆叠。
- 不做厚重阴影。
- 不做大面积渐变光效。
- 不把 Settings 变成控制台。
- General 只放用户最容易理解的核心项。
- 高风险或高复杂功能放 Advanced。
- Ask 和 Capsule 都要轻，像系统级辅助层，不像一个新 app。

### 3.2 行为原则

- 用户明确触发，系统才行动。
- 不偷偷学习。
- 不偷偷截图。
- 不自动替换用户原文。
- AI 结果默认可复制，由用户自己决定是否粘贴。
- 稳定性优先于“看起来很酷”。

### 3.3 对竞品的借鉴边界

OpenLess 值得借鉴：

- 胶囊状态流畅度。
- Raw / Light / Structured / Formal 这类基础 polish mode。
- 插入失败时的回退思路。
- 词典和纠错对 ASR/polish 的帮助。

Typeless 值得借鉴：

- `Fn` / `RightAlt` 主听写 hotkey。
- Ask 是独立 hotkey。
- 选中文本后用语音命令处理这段文字。
- 个人词典和输出风格要让用户容易理解。

闪电说本期只借鉴一个方向，但不落地：

- “看当前上下文再回答”是有价值的。
- 但截图上下文 token 消耗大，放到下一期，并且必须显式触发和额度保护。

## 4. 本期范围总表

| 模块 | 本期做什么 | 优先级 | UI 改动级别 | 风险 | 实施切片 |
| --- | --- | --- | --- | --- | --- |
| Polish Style | Minimal/Clean/Structured/Professional 四模式 | P0 | 小 | 中 | A |
| Dictionary | Words + Corrections | P1 | 小 | 中 | A |
| Ask | 便签式 Ask、选中文本上下文、Copy only | P0 | 中小 | 中 | B |
| Capsule | 轻量状态流畅度、阴影收敛、录音/思考反馈 | P0 | 小 | 低 | B |
| Streaming insertion | 稳定性、失败回退、history 完整性 | P1/P2 | 极小 | 中 | B |
| Screen context | 下一期，只写边界 | Next | 无 | 高成本 | Later |
| Local ASR model manager | 暂不做 | P2 later | 无 | 高 | Later |
| Marketplace / Agent | 不做 | Out | 无 | 高 | Never in this scope |

## 5. Feature 1: 胶囊轻量优化

### 5.1 目标

当前胶囊不是坏的，功能已经有。问题是状态变化还可以更自然。

本期目标不是重做胶囊，而是让它从“状态提示”变成“顺滑的语音过程反馈”。

### 5.2 用户体验

用户按下听写 hotkey 后：

1. 胶囊轻轻出现。
2. 录音时有明确“正在听”的感觉。
3. 松开后自然过渡到 transcribing。
4. 如果开启 AI polish，自然进入 thinking/polishing。
5. 输出完成后短暂完成反馈，然后自然消失。

用户不应该看到：

- 大片灰色阴影。
- 突然闪现/闪没。
- 状态硬切。
- 无意义长文案。
- 花哨光效。

### 5.3 状态定义

保留现有 pipeline 状态，不引入复杂新状态：

- `idle`
- `preparing`
- `recording`
- `transcribing`
- `polishing`
- `outputting`
- `error`

UI 需要重点优化状态之间的桥：

- `idle -> preparing`
- `preparing -> recording`
- `recording -> transcribing`
- `transcribing -> polishing`
- `polishing -> outputting`
- `outputting -> idle`

### 5.4 UI 要求

- 保留当前胶囊的整体尺寸和形态。
- 背景阴影要明显减轻。
- 录音态可以有轻微 waveform / breathing。
- transcribing / polishing 可以有细小 dots / orb / pulse，但不能抢眼。
- error 状态要清楚，但不能做成大警告弹窗。
- 完成态只短暂停留，不长期占屏。

### 5.5 代码参考

重点文件：

- `src/components/Capsule/index.tsx`
- `src/components/Capsule/CapsuleRecording.tsx`
- `src/components/Capsule/CapsuleTranscribing.tsx`
- `src/components/Capsule/CapsulePolishing.tsx`
- `src/components/Capsule/Waveform.tsx`
- `src/hooks/useRecording.ts`
- `src/stores/appStore.ts`

### 5.6 验收标准

- 按下 hotkey 后胶囊出现不突兀。
- 录音态用户一眼知道系统正在听。
- transcribing/polishing 不像卡住。
- 输出结束后自然消失。
- Ask/Capsule 背后没有一大片沉重阴影。
- 不改变主窗口 UI。
- 不新增复杂配置项。

### 5.7 测试建议

手动验收：

- macOS: `Fn` 听写一次短句。
- macOS: `Fn` 听写一段长句并开启 AI polish。
- 异常路径：没有麦克风权限 / STT 失败 / LLM 失败。
- 截图记录 idle、recording、transcribing、polishing、done。

自动测试：

- 保留现有 Capsule render tests。
- 可增加 state transition smoke test，确认不同 pipeline state 不崩。

## 6. Feature 2: Ask Anything 极简便签

### 6.1 目标

Ask 不再只是“问一句，答一句”。

本期要把 Ask 做成 Typeless 更接近的体验：

> 没选中文字时，是普通语音问答。
> 选中文字时，是对这段文字说命令，让 AI 处理这段文字。

结果仍然显示在 Ask 便签里，并且只提供 Copy。

### 6.2 Hotkey

保持当前方向：

- macOS Ask: `Fn+Space`
- Windows Ask: `RightAlt+Space`

听写 hotkey 和 Ask hotkey 不合并。

### 6.3 无选中文字流程

用户流程：

1. 用户按 Ask hotkey。
2. 出现轻量便签。
3. 用户说问题。
4. 系统转写问题。
5. LLM 回答。
6. 结果显示在便签里。
7. 用户点击 Copy，或者点外面关闭。

### 6.4 有选中文字流程

用户流程：

1. 用户在任意 app 里选中一段文字。
2. 用户按 Ask hotkey。
3. 便签出现。
4. 便签里只显示轻量上下文提示，例如：
   - `Selected text`
   - `Using selected text`
   - 或者 `已选中文本`
5. 用户说命令：
   - “总结一下”
   - “翻译成英文”
   - “改得更自然”
   - “解释这段”
   - “缩短一点”
6. LLM 基于选中文字 + 用户语音命令生成结果。
7. 便签里显示结果。
8. 只提供 Copy。

### 6.5 Copy only 决策

本期只保留 Copy，不做 Replace / Insert。

原因：

- Replace 可能误改用户原文。
- Insert 对跨 app 焦点要求高，容易让用户困惑。
- Copy 最稳定，用户自己决定在哪里粘贴。
- UI 更极简。

### 6.6 关闭行为

便签关闭方式：

- 点击外部区域关闭。
- Esc 关闭。
- 点击右上角关闭。

关闭时：

- 如果请求还在进行，应 abort 或忽略 pending result。
- 不应该在用户关闭后突然更新 UI。

### 6.7 UI 要求

Ask 便签应该像轻量便签，不像大聊天窗口。

要求：

- 宽度克制。
- 背景干净。
- 阴影轻。
- 不要大片灰雾。
- 不放聊天历史。
- 不放多个复杂按钮。
- 结果区域可滚动，但不出现奇怪横向滚动条。
- Copy 是唯一主操作。

### 6.8 代码参考

重点文件：

- `src/components/AskPanel/AskPanel.tsx`
- `src-tauri/src/commands/ask.rs`
- `src-tauri/src/selection.rs`
- `src-tauri/src/hotkey.rs`
- `src-tauri/src/native_hotkey.rs`
- `src/hooks/useTauriEvents.ts`
- `src/stores/appStore.ts`

### 6.9 验收标准

无选中文字：

- Ask hotkey 能打开便签。
- 用户语音问题能被转写。
- LLM 结果显示在便签里。
- Copy 能复制完整结果。
- 点击外部关闭。

有选中文字：

- 系统能读取选中文本上下文。
- 用户说“总结一下”，结果确实总结选中文字。
- 用户说“翻译成英文”，结果确实翻译选中文字。
- 结果只显示在便签里，不自动替换原文。
- Copy 能复制完整结果。

失败路径：

- 选中文本读取失败时，Ask 仍可作为普通问答使用。
- STT 失败时显示轻量错误。
- LLM 失败时显示轻量错误。
- 用户关闭后，不再弹回结果。

## 7. Feature 3: Polish Style 四模式

### 7.1 目标

把用户难以理解的 Scenes/Prompt 心智，收敛成更好懂的：

> Polish style

它的意思就是：

> AI polish 的结果应该长什么样。

### 7.2 四个基础模式

本期新增或显式化四个基础输出模式：

| OpenTypeless 名称 | 对应 OpenLess | 说明 |
| --- | --- | --- |
| Minimal | Raw | 尽量保留原话，只补标点和必要分句 |
| Clean | Light polish | 默认，去口癖、补标点、轻度润色 |
| Structured | Structured | 多事项自动整理成分点、编号、主题 |
| Professional | Formal | 适合工作沟通和邮件，更正式但不油腻 |

默认值：

- `Clean`

不默认 `Structured`，因为日常输入每次都自动分点会显得太重。

### 7.3 General UI

General 里最多出现一行：

```text
Polish style
Clean
```

可选项：

```text
Minimal
Clean
Structured
Professional
```

不要在 General 里展示 prompt。
不要在 General 里展示 scene 列表。
不要在 General 里出现“Style Pack / Marketplace / Prompt Template”这些开发者心智词。

### 7.4 Scenes 的处理

现有 Scenes 不删除。

但产品心智上：

- General 里的主入口叫 `Polish style`。
- Scenes 保留为高级自定义风格。
- Scenes 不在本期升级成 OpenLess Marketplace。
- 自定义 scene 继续可导入/导出，但不要把它推到普通用户面前。

### 7.5 Prompt 行为

每个模式应该有独立 prompt 规则。

共同规则：

- 不回答转写中的问题。
- 不执行转写中的命令。
- 不添加用户没说的事实。
- 不输出“以下是整理后的内容”。
- 中英混排、路径、代码、URL、版本号、产品名尽量保留。
- 以最终改口为准。

模式差异：

Minimal:

- 只补标点和断句。
- 尽量不改词。
- 不重排结构。
- 可去掉明显口癖。

Clean:

- 去口癖。
- 补标点。
- 小幅调整语序。
- 保留语气。
- 输出可直接发送的自然文字。

Structured:

- 当有 2 个以上事项时，倾向编号。
- 当有 3 个以上事项时，按主题归类。
- 不丢事项。
- 不强行扩写。
- 不照抄混乱顺序。

Professional:

- 适合工作沟通、邮件、跨团队同步。
- 更克制、清楚、完整。
- 不加空泛客套。
- 不把一句话扩成商务长文。

### 7.6 数据模型要求

本期采用显式字段，不再只靠 built-in scene 伪装 style。原因是 style 是主路径，scene 是高级自定义；两者需要清楚分层。

1. 新增 `polish_style` 字段，值为：
   - `minimal`
   - `clean`
   - `structured`
   - `professional`
2. 保留 `active_scene`。
3. prompt 组合优先级：
   - active custom scene 存在时，作为更高优先级自定义规则。
   - 否则使用 `polish_style` 对应内置 prompt。
   - `polish_custom_prompt` 作为追加规则保留。

不采用“只用 built-in scenes 承载四个基础模式”的方案。那样会继续混淆 Scenes 和 Polish Style 的心智。

### 7.7 代码参考

重点文件：

- `src-tauri/src/storage/mod.rs`
- `src-tauri/src/llm/prompt.rs`
- `src-tauri/src/llm/openai.rs`
- `src-tauri/src/pipeline.rs`
- `src/stores/appStore.ts`
- `src/components/Settings/GeneralPane.tsx`
- `src/components/Settings/LlmPane.tsx`
- `src/components/Settings/ScenesPane.tsx`
- `src/lib/scenes/builtinScenes.ts`
- `src/i18n/locales/*.json`

### 7.8 验收标准

- 新用户默认 `Clean`。
- General 能看到并切换 Polish style。
- Minimal 输出接近原话。
- Clean 输出自然、去口癖。
- Structured 对多事项输入能分点整理。
- Professional 对邮件/工作沟通更正式但不扩写。
- 切换 style 后下一次听写生效。
- active custom scene 不被破坏。
- 旧配置迁移后不崩。

### 7.9 测试样例

Minimal 输入：

```text
嗯那个我刚刚跟客户聊完然后他说下周三可以给反馈
```

期望方向：

```text
我刚刚跟客户聊完，他说下周三可以给反馈。
```

Clean 输入：

```text
那个我觉得这个方案吧大概可以但是可能在性能上还要再看看
```

期望方向：

```text
我觉得这个方案大概可以，但性能上还要再看看。
```

Structured 输入：

```text
帮我整理一下今天要做的事情首先修一下登录然后看一下支付失败的问题还有 README 要更新一下另外把前端构建跑一遍
```

期望方向：

```text
今天需要处理以下事项：

1. 功能修复
   (a) 修复登录问题。
   (b) 排查支付失败问题。
2. 文档与构建
   (a) 更新 README。
   (b) 跑一遍前端构建。
```

Professional 输入：

```text
老张你好昨天发你的合同你看了没我们这边比较急想问下什么时候能反馈
```

期望方向：

```text
老张，你好：

昨天发您的合同是否已查阅？我们这边较为着急，想确认一下您预计什么时候可以反馈。
```

## 8. Feature 4: Dictionary Words + Corrections

### 8.1 目标

让 OpenTypeless 更懂用户明确告诉它的词和纠错规则。

本期只做两类：

1. Words: 重要词。
2. Corrections: 错误词 -> 正确词。

不做自动学习。
不做知识库。
不做写作风格记忆。

### 8.2 Words

Words 是用户主动添加的重要词。

例子：

- OpenTypeless
- TalkMore
- Qwen3
- App ID
- Access Token

作用：

- 转写/polish 时优先保留。
- 不要乱翻译。
- 不要改大小写。
- 可以作为热词进入 prompt。

### 8.3 Corrections

Corrections 是简单纠错规则。

例子：

```text
拓肯 -> Token
克劳德 -> Claude
跟目录 -> 根目录
西克瑞特 Key -> Secret Key
```

第一版只做普通字符串，不做正则。

原因：

- 用户更容易理解。
- 风险低。
- UI 简单。
- 不会出现正则误伤。

### 8.4 UI

Dictionary 页面分两块即可：

```text
Words
Corrections
```

Words:

- 输入词。
- 可选备注。
- enabled toggle 可保留。

Corrections:

- Wrong phrase 输入框。
- Correct phrase 输入框。
- Add。
- 每条可启用/停用/删除。

不要新增复杂 tabs。
不要加自动学习弹窗。
不要把 Dictionary 放到 General。

### 8.5 Prompt 集成

Corrections 推荐进入 polish prompt，而不是简单前端替换。

原因：

- AI 可以结合上下文判断。
- 避免机械替换破坏正常句子。
- 可以和 Words 同一套热词模块合并。

Prompt 语义：

```text
When the transcript likely contains "拓肯", output "Token".
When the transcript likely contains "跟目录" in a file-system context, output "根目录".
```

### 8.6 数据模型建议

如果当前后端已有 correction rule 类型，沿用现有结构。

否则新增：

```ts
interface CorrectionRule {
  id: string
  pattern: string
  replacement: string
  enabled: boolean
  created_at: string
}
```

边界：

- `pattern` 必须非空。
- `replacement` 必须非空。
- 单条长度限制。
- 总数量限制，避免 prompt 膨胀。
- prompt 只注入 enabled 的前 N 条。

### 8.7 代码参考

重点文件：

- `src-tauri/src/storage/mod.rs`
- `src-tauri/src/llm/prompt.rs`
- `src/components/Settings/DictionaryPane.tsx`
- `src/stores/appStore.ts`
- `src/i18n/locales/*.json`

需要先确认当前 Dictionary 实现具体文件名，如果不是 `DictionaryPane.tsx`，以实际文件为准。

### 8.8 验收标准

- 用户可以添加 Word。
- 用户可以添加 Correction。
- Correction 能启用/停用/删除。
- Correction 会进入 polish 请求。
- `拓肯` 这类测试输入能稳定输出 `Token`。
- 不影响现有 Words。
- 空输入不能保存。
- 过长输入会被限制。
- 迁移旧配置不崩。

## 9. Feature 5: Streaming insertion 稳定性

### 9.1 目标

Streaming insertion 本期不作为主功能推广，不默认开启。

本期只确认：

- 失败不丢结果。
- History 保存完整结果。
- 焦点不可信时不继续乱打字。
- 回退路径明确。

### 9.2 默认策略

- 默认关闭。
- 放 Advanced。
- 不放 General。
- 文案不要过度营销。

可以叫：

```text
Type as text is generated
```

或者中文理解：

```text
生成时直接打出来
```

### 9.3 失败回退

如果 streaming 中途失败：

1. LLM 最终成功返回完整结果：
   - History 保存完整结果。
   - 如果能安全补 suffix，则补 suffix。
   - 如果不能确认目标 app/focus，则不继续打字。
   - 完整结果放剪贴板。
2. LLM 失败：
   - 不把 partial 当作最终结果。
   - History 记录失败。
   - 用户能看到轻量错误。
3. 插入失败：
   - 完整结果复制到剪贴板。
   - 不清空用户原剪贴板，或在可恢复时恢复 text-only snapshot。

### 9.4 验收标准

- 默认配置下 streaming off。
- 开启 streaming 后短文本可逐步插入。
- 长文本中途失败时，完整结果可从剪贴板拿到。
- History 不保存半截为成功结果。
- 焦点变化时不会继续往错误 app 输入。

### 9.5 代码参考

重点文件：

- `src-tauri/src/pipeline.rs`
- `src-tauri/src/output/*`
- `src-tauri/src/storage/mod.rs`
- `src/components/Settings/GeneralPane.tsx`
- `src/components/Settings/Advanced*`

## 10. 下一期: Ask with screen context

### 10.1 本期结论

截图上下文有价值，但不进本期。

原因：

- 多模态截图 token/费用明显高于文字。
- 云端用户每月额度可能很快被打爆。
- 如果默认开启，成本不可控。
- 截图也涉及隐私，需要非常明确的触发和提示。

### 10.2 下一期原则

如果下一期做，必须遵守：

- 默认关闭。
- 不后台截图。
- 只在用户明确触发 Ask with screen 时截图。
- 普通听写不截图。
- 选中文字优先于截图。
- 不保存截图到 History。
- 第一次使用提示“Screen context uses more credits.”
- 云端用户要有限额或更高权重扣额度。
- 自带 key 用户可以更自由，但仍提示成本。
- 截图必须压缩，不传原始 5K 全屏。

### 10.3 下一期可能 UI

Ask 便签里放一个很轻的小相机按钮，或者独立命令：

```text
Ask with screen
```

不做默认自动带截图。

## 11. 明确不做

本期不做以下内容：

### 11.1 不做 OpenLess Marketplace

不做：

- style pack marketplace
- publish / like / browse community packs
- pack moderation
- pack icon/assets
- one-click install cloud packs

原因：

- 对当前 OpenTypeless 太重。
- 会污染 Settings 心智。
- 不是当前最大体验短板。

### 11.2 不做完整 Style Pack 系统

不做：

- 复杂 metadata。
- recommended model。
- compatible version。
- 多级 tags。
- ZIP 包资产管理。
- 风格包热键切换。

只做 Polish Style 四模式 + 保留现有 custom scenes。

### 11.3 不做闪电说式沟通 Agent

不做：

- 知识库。
- 常用回复。
- 自动生成聊天回复。
- 自动读取聊天上下文。
- 工具调用。
- 任务执行。
- RAG。

原因：

- 产品定位会跑偏。
- 隐私风险上升。
- UI 和额度复杂度都会上升。

### 11.4 不做本地 ASR 模型管理

不做：

- 模型下载。
- 模型删除。
- 多本地模型切换。
- Qwen3/Foundry/Sherpa 管理 UI。
- 模型体积/路径/状态面板。

保留现有：

- Apple Speech。
- Cloud STT。
- Custom Whisper endpoint。

后续 P2 再单独设计。

### 11.5 不做自动学习

不做：

- 自动学习用户写作风格。
- 自动加入词典。
- 自动上传用户内容训练。
- 黑盒“越用越懂你”。

本期只做用户明确输入的 Words 和 Corrections。

## 12. 设置页信息架构

### 12.1 General

General 应该只保留用户最常用、最好懂的项。

建议包含：

- Launch at login
- Dictation hotkey
- Ask hotkey
- AI polish on/off
- Polish style
- Output behavior 的极少量主项

不要把以下放 General：

- Streaming insertion 细节。
- prompt 编辑器。
- model management。
- marketplace。
- correction rules。
- screen context。

### 12.2 AI Polish

AI Polish 页可以承载：

- provider/model 设置。
- custom instructions。
- active custom scene 的轻量提示。

但不要让普通用户必须理解 prompt。

### 12.3 Dictionary

Dictionary 承载：

- Words。
- Corrections。

### 12.4 Scenes / Custom Styles

Scenes 保留，但产品文案可以逐步弱化为：

- Custom styles
- Advanced polish styles

不建议继续把它作为普通用户主路径。

## 13. 技术实施顺序

推荐分 4 个 batch。

### Slice A / Batch 1: Polish Style 四模式

原因：

- 需求清晰。
- 对用户心智帮助大。
- 便于后续 Ask/Dictionary 复用 prompt。

任务：

1. 增加 `polish_style` 配置或等价映射。
2. 增加四个内置 prompt。
3. AI Polish 设置区增加极简下拉，避免 General 继续变重。
4. 确认 active custom scene 兼容。
5. 增加 prompt 单元测试。

### Slice A / Batch 2: Dictionary Corrections

任务：

1. 增加 CorrectionRule 数据和 SQLite 表。
2. Dictionary UI 增加 Corrections。
3. prompt 注入 enabled corrections。
4. 边界限制和迁移。
5. 增加 storage/prompt/UI tests。

### Slice B / Batch 3: Ask 便签 + Copy only

Status: Ask 便签主体已落地；本次补齐 Ask hotkey 启动后的录音中间态、选中文本提示、pending recordingStarted 兜底、失焦/关闭中止录音。

任务：

1. 调整 Ask UI。
2. 选中文字时传 selected text context。
3. 结果只提供 Copy。
4. 点击外部关闭。
5. pending request 关闭后不回填。
6. Ask hotkey 录音开始后立刻打开便签并显示录音态。
7. native event 丢失时，pending recordingStarted 能恢复便签状态。
8. 增加 Ask UI 测试和手动截图验收。

### Slice B / Batch 4: Capsule + Streaming 稳定性

任务：

1. 胶囊状态过渡和阴影轻量优化。
2. Capsule 手动截图验收。
3. Streaming 回退路径确认。
4. History 完整结果确认。
5. 插入失败 fallback 验收。

## 14. 端到端验收清单

### 14.1 基础听写

- macOS `Fn` 能启动/停止听写。
- Windows `RightAlt` 能启动/停止听写。
- 结果能插入当前输入框。
- 胶囊状态完整经过 recording -> transcribing -> outputting。

### 14.2 Polish Style

- Minimal 生效。
- Clean 生效。
- Structured 生效。
- Professional 生效。
- 切换后下一次听写生效。
- 默认是 Clean。

### 14.3 Ask

- `Fn+Space` / `RightAlt+Space` 打开 Ask。
- 按下 Ask hotkey 后，便签立刻显示录音态，不等结果回来才出现。
- 无选中文字可以普通问答。
- 有选中文字可以总结。
- 有选中文字可以翻译。
- 有选中文字可以改写。
- 有选中文字录音时，便签显示轻量提示，不展示原文内容。
- Copy 复制完整结果。
- 点击外面关闭。
- Ask 录音中点击外面/失焦会中止录音。
- 关闭后 pending result 不回弹。

### 14.4 Dictionary

- Word 可添加。
- Correction 可添加。
- Correction 可停用。
- Correction 可删除。
- Correction 影响 polish 输出。
- 空值/过长值不保存。

### 14.5 Streaming

- 默认关闭。
- 开启后可 streaming。
- 中途失败不丢完整结果。
- History 保存完整结果。
- 焦点变化不乱打字。

### 14.6 UI

- General 没有变复杂。
- Ask 没有厚重阴影。
- Dictionary 没有横向粗滚动条。
- Settings 没有出现大面积新增复杂文案。
- 主窗口整体仍然简洁、高级。

## 15. 风险和缓解

### Risk 1: Polish Style 和 Scenes 冲突

风险：

- 用户已有 active scene，不知道它和 Polish style 谁优先。

缓解：

- 自定义 active scene 优先。
- General 显示当前 style。
- AI Polish / Scenes 页显示 active custom style。
- 文案保持简短，不解释一大段。

### Risk 2: Structured 过度格式化

风险：

- 短句也被强行分点，显得很重。

缓解：

- Structured prompt 明确：1 个事项输出段落，2 个以上才编号。
- 默认用 Clean。

### Risk 3: Ask 读取选中文字不稳定

风险：

- 跨 app 选中文本获取失败。

缓解：

- 获取失败时退回普通 Ask。
- 不阻塞 Ask 打开。
- 便签里轻量提示，不弹大错误。

### Risk 4: Correction 误伤

风险：

- 简单字符串规则可能把本来正确的词改错。

缓解：

- 第一版通过 prompt 注入，让模型结合上下文。
- 允许停用规则。
- 不做全局机械替换。

### Risk 5: Streaming 半截插入

风险：

- 用户看到半截文本，完整结果丢失。

缓解：

- 完整结果进 history。
- 完整结果进剪贴板 fallback。
- 不把 partial 当成功。

## 16. 成功标准

本期完成后，应该达到：

- 用户能在 General 一眼理解 AI polish 的输出风格。
- Ask 选中文本后能自然处理，不需要大窗口。
- Dictionary 不只是词表，还能处理常见误识别。
- 胶囊看起来更顺，但没有变花哨。
- 默认体验仍然极简。
- 没有把 OpenTypeless 做成复杂 Agent。

## 17. 最终本期结论

可以补齐的差异：

1. 胶囊质感。
2. Ask 选中文本工作流。
3. Polish Style 四模式。
4. Dictionary corrections。
5. Streaming 回退稳定性。

暂不补齐的差异：

1. 本地 ASR 模型管理。
2. Style Pack Marketplace。
3. 闪电说知识库/Agent。
4. 截图上下文。
5. 自动学习。

下一步如果继续收口，建议聚焦 `Capsule + Streaming` 的人工端到端验收：确认胶囊状态变化、失败回退、history 完整结果和截图记录。
