# Ask Anything Floating Note UX Spec

## 背景

当前 Ask Anything 已经有独立快捷键、语音提问、回答展示、复制答案、搜索意图、选中文本上下文等能力，但 UI 形态还像一个小面板或设置页里的嵌入组件。

这和 OpenTypeless 当前的产品气质不完全一致。OpenTypeless 更像一个轻量系统工具：唤起快、状态清楚、做完就离开，不应该让 Ask Anything 看起来像一个需要管理的聊天窗口。

本 spec 的目标是把 Ask Anything 调整成“便签式临时浮窗”：轻、安静、贴近当前上下文，问完就可以自然消失。

## 目标

1. Ask Anything 的独立弹窗要像一张高级极简便签，而不是完整面板。
2. 点击便签外部区域后，便签应关闭或取消当前 Ask 流程。
3. UI 必须和当前 OpenTypeless 风格对齐：低噪音、小字号、轻边框、柔和阴影、少解释。
4. 不引入聊天历史、复杂输入框、大标题栏、营销式说明。
5. 保留 Settings 里嵌入 Ask 的能力，但嵌入态不应该完全照搬浮窗态。

## 非目标

1. 不重做整个 Ask 功能架构。
2. 不把 Ask Anything 改成聊天机器人。
3. 不新增多轮对话历史。
4. 不大改主设置页视觉。
5. 不改变 AI provider、cloud quota、BYOK、本地 provider 的业务逻辑。
6. 不解决 Google/GitHub OAuth provider 配置问题。

## 产品定位

Ask Anything 是一个“临时问答工具”，不是一个长期停留的工作区。

用户心理模型应该是：

1. 我按 Ask 快捷键。
2. 它像便签一样浮出来。
3. 我说一句问题。
4. 它给一个短答案。
5. 我复制、参考，或者点外面让它消失。

这和普通 dictation 的区别是：

1. Dictation 负责把话变成文本并插入当前 app。
2. Ask Anything 负责回答一个问题，不应该默认插入当前 app。
3. Ask Anything 的结果是“临时参考”，所以 UI 应该短暂停留，而不是像主窗口一样存在。

## 视觉方向

### 形态

Ask 独立窗口应变成 floating note：

1. 宽度：默认 380px，允许根据平台在 360-420px 之间微调。
2. 高度：内容自适应，默认最小 160px，结果较长时最大 420px，内部滚动。
3. 位置：默认在屏幕中央略偏上，或者靠近胶囊/当前鼠标附近。如果定位成本高，第一版使用居中偏上即可。
4. 圆角：8-12px，不能超过当前项目整体圆角太多。
5. 阴影：轻阴影，不要强烈玻璃拟态，不要彩色光晕。
6. 边框：使用现有 `border` token，低对比。
7. 背景：使用现有 `bg-elevated` 或 `bg-primary`，不要新建大面积颜色体系。

### 内容密度

浮窗态只保留必要内容：

1. 顶部状态行：`Ask` / `Listening` / `Thinking` / `Answer`
2. 录音/停止按钮：图标优先，文本极短。
3. 问题文本：最多 2-3 行，长文本折叠或滚动。
4. 答案文本：主内容。
5. 操作：Copy、Close。必要时保留 Retry。

不显示：

1. 长说明文案。
2. 大块帮助文本。
3. 复杂标题栏。
4. 多个卡片嵌套。
5. “像聊天 app 一样”的消息列表。

## 交互规则

### 打开

触发来源：

1. Ask 快捷键。
2. Settings 里的 Ask 预览按钮。
3. Rust 后端收到 `ask:result` 或 `ask:error` 后调用 Ask 窗口。

独立浮窗打开时：

1. 如果窗口不存在，创建。
2. 如果窗口已存在但隐藏，显示并聚焦。
3. 如果窗口已显示，再次触发 Ask 快捷键应开始新的 Ask 流程，而不是堆一个新窗口。

### 关闭

关闭方式：

1. 点击便签外部：关闭。
2. 按 Esc：关闭。
3. 点击 Close 图标：关闭。
4. 系统关闭窗口：隐藏窗口，保持后续可复用。

不同状态下的关闭规则：

| 状态 | 点击外部/按 Esc 的行为 |
| --- | --- |
| idle | 隐藏便签 |
| recording | abort 当前 Ask 录音，然后隐藏 |
| processing/thinking | 标记当前 Ask UI 已放弃，隐藏；后续结果不应重新弹出打扰用户 |
| result | 隐藏便签 |
| error | 隐藏便签 |

重点：用户点击外部后，Ask 不应该几秒后因为异步结果又自己弹回来。

### 录音

录音态视觉：

1. 状态显示 `Listening`。
2. 使用一个小红点或 accent 点轻微 pulse。
3. 停止按钮用 `Square` 或 Stop 图标。
4. 不显示大面积录音动画。

停止录音后：

1. 状态变为 `Thinking`。
2. 按钮 disabled 或变成小 loading。
3. 问题文本可以显示最终转写。
4. 不允许重复触发多个 Ask 请求。

### 回答

回答出现时：

1. 状态变为 `Answer` 或直接显示 `Ask`。
2. 显示问题和答案。
3. Copy 按钮可见。
4. 点击 Copy 后显示极短反馈：`Copied`，1.5 秒后消失。
5. 长答案在便签内部滚动，不扩大到屏幕大面板。

回答长度建议：

1. Rust/LLM prompt 已限制短答案，UI 不应该鼓励长文。
2. 如果答案超过最大高度，内部滚动。
3. 滚动条要细、低存在感，不能出现 Dictionary 那种粗横向滚动条问题。

### 错误

错误态：

1. 保持便签形态，不跳大错误页。
2. 文案要短，例如 `Could not answer. Check your connection or provider settings.`
3. 提供 Retry 或 Close。
4. 不展示技术堆栈。

## 嵌入态和浮窗态分离

当前 `AskPanel` 同时服务 embedded 和 standalone。下一步应明确分离视觉责任：

1. `AskPanel` 保留核心状态和逻辑，或拆出 shared hook。
2. `AskFloatingNote` 负责独立浮窗 UI。
3. `AskEmbeddedPanel` 负责 Settings 内嵌预览。

第一版可以不完全拆文件，但必须在代码结构上区分：

1. 非 embedded 不再使用 `h-screen w-screen overflow-y-auto bg-bg-primary` 这种全屏面板形态。
2. embedded 可以继续在 Settings 里作为轻量测试入口，但要去掉复杂说明。

## 具体 UI 结构

建议浮窗 DOM 结构：

```tsx
<div className="ask-note-shell">
  <header className="ask-note-header">
    <div className="ask-note-status">
      <span className="ask-note-dot" />
      <span>Listening</span>
    </div>
    <button aria-label="Close">...</button>
  </header>

  <main className="ask-note-body">
    <section className="ask-note-question">...</section>
    <section className="ask-note-answer">...</section>
  </main>

  <footer className="ask-note-actions">
    <button>Copy</button>
  </footer>
</div>
```

视觉原则：

1. Header 高度不超过 36px。
2. Body padding 12-14px。
3. 字号：状态 11-12px，问题 12px，答案 13px。
4. 行高：答案 1.55 左右。
5. 图标按钮 28px 左右，不能做大。

## 外部点击关闭实现建议

Tauri 独立窗口通常无法自然接收“窗口外点击”的 DOM 事件，因为窗口外部不属于当前 WebView。

因此关闭策略建议分两层：

### 第一层：窗口失焦关闭

监听 Tauri window focus/blur：

1. Ask 窗口失焦时，如果状态是 `idle/result/error`，隐藏窗口。
2. 如果状态是 `recording`，先 abort，再隐藏。
3. 如果状态是 `processing`，隐藏并标记当前结果不再自动弹出。

这最接近“点击外部关闭”的真实桌面语义。

### 第二层：窗口内部空白点击关闭

如果后续采用透明/overlay 式窗口，可以在 note 外层加 overlay，并监听 overlay click。

第一版不建议做全屏透明 overlay，因为：

1. 会更像 modal，不够轻。
2. 可能挡住用户当前 app。
3. 跨平台行为更复杂。
4. 和 less is more 原则冲突。

所以第一版用“失焦关闭”更稳。

## Tauri 窗口建议

当前 Ask 窗口：

```rust
.inner_size(420.0, 320.0)
.min_inner_size(360.0, 260.0)
.resizable(true)
.always_on_top(true)
.skip_taskbar(true)
.center()
```

建议第一版调整：

1. `inner_size(380.0, 220.0)`。
2. `min_inner_size(340.0, 140.0)`。
3. `max height` 由前端内容滚动控制；如需 Rust 控制，再做动态 resize。
4. `resizable(false)`，让它更像便签而不是窗口。
5. `always_on_top(true)` 保留。
6. `skip_taskbar(true)` 保留。
7. `decorations(false)` 可考虑，但需验证 macOS/Windows/Linux 的拖拽和关闭体验。第一版可以先保留 decorations 或使用轻标题栏，不要冒险破坏窗口操作。

如果去掉 decorations，必须提供：

1. 可拖拽区域。
2. Close 按钮。
3. Esc 关闭。
4. 可访问的按钮 aria-label。

## 状态机

Ask UI 状态建议统一成：

```ts
type AskNoteState =
  | 'idle'
  | 'recording'
  | 'thinking'
  | 'result'
  | 'error'
  | 'dismissed'
```

状态转换：

1. `idle -> recording`：用户按 Ask 快捷键或点击录音。
2. `recording -> thinking`：用户停止录音。
3. `thinking -> result`：收到 `ask:result`。
4. `thinking -> error`：收到 `ask:error`。
5. `recording -> dismissed`：用户点击外部 / Esc / Close。
6. `thinking -> dismissed`：用户点击外部 / Esc / Close。
7. `result -> dismissed`：用户点击外部 / Esc / Close。
8. `error -> dismissed`：用户点击外部 / Esc / Close。
9. `dismissed -> recording`：再次按 Ask 快捷键。

关键规则：

如果状态是 `dismissed`，后续迟到的 `ask:result` 或 `ask:error` 不应该自动显示窗口。

## Settings 中的 Ask

Settings 里的 Ask Anything 不应该展示完整便签，而应该作为“测试入口”：

1. 标题：`Ask Anything`
2. 一行说明：`Ask a one-off question by voice.`
3. 一个小按钮：`Try Ask`
4. 当前快捷键展示：例如 `Fn + Space` / `Right Alt + Space`

用户点击 Try Ask：

1. 可以直接打开 floating note。
2. 不需要在 Settings 页面内塞完整回答区域。

这样 Settings 会更干净，也符合用户之前反馈“General 内容太多，看不懂”。

## 验收标准

### 视觉验收

1. Ask 独立窗口看起来像轻量便签，不像设置页。
2. 没有大块说明文案。
3. 没有卡片套卡片。
4. 没有复杂颜色和大面积 accent。
5. 字号、边距、圆角和当前 OpenTypeless 风格一致。
6. 长答案只在内部纵向滚动，不出现横向滚动条。

### 交互验收

1. Ask 快捷键能打开便签。
2. 录音态能显示 Listening。
3. 停止后能显示 Thinking。
4. 成功后能显示问题和答案。
5. Copy 能复制答案，并显示短反馈。
6. Esc 能关闭便签。
7. 点击窗口外部能关闭便签，或至少窗口失焦能关闭。
8. recording 状态关闭会 abort，不继续录音。
9. thinking 状态关闭后，迟到结果不会重新弹窗。
10. 再次按 Ask 快捷键能开始新一轮。

### 回归验收

1. Settings 页面不变复杂。
2. 主 dictation 胶囊不受影响。
3. AI Polish scenes 不受影响。
4. Dictionary 不受影响。
5. Ask hotkey 仍能保存和校验冲突。
6. Cloud/BYOK Ask 逻辑不变。

## 建议实施顺序

1. 先改 `AskPanel` 非 embedded UI，让它从全屏面板变成便签。
2. 再加 Esc 关闭和关闭按钮。
3. 再接 Tauri window blur，实现点击外部/失焦隐藏。
4. 再处理 dismissed 状态，避免迟到结果重新弹出。
5. 最后收敛 Settings 内嵌 Ask，只保留 Try Ask 入口。

## 需要用户确认的点

1. 便签是否允许用户拖动？
   - 推荐：第一版允许拖动，位置不持久化。
2. 是否去掉系统窗口标题栏？
   - 推荐：先不强行去掉；如果当前标题栏太突兀，再做 decorations false。
3. 点击外部关闭时，thinking 中的请求是否彻底取消？
   - 推荐：UI 先关闭并忽略结果；真正 cancel 后端请求可以后续再补。
4. Settings 里是否直接移除嵌入式完整 AskPanel？
   - 推荐：移除完整面板，保留 Try Ask。
