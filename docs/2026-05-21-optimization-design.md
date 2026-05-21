# OpenTypeless 优化设计文档

## 背景

OpenTypeless 是语音转文字桌面应用（Tauri + React + Rust），当前 ~500 用户，v0.1.0 阶段。存在 9 个 open issue 影响用户体验，代码层面有技术债需要处理。

## 方案

采用三阶段逐步优化：安装体验 → 流水线可靠性 → 代码重构。每阶段独立可交付，风险可控。

---

## 阶段 1：安装体验修复

### Windows (#35, #37)

- README 和 GitHub Release 页面添加安装说明：右键 → 属性 → 解除锁定（或运行 → 更多信息 → 仍要运行）
- 说明这是缺少代码签名导致的 SmartScreen 警告
- 考虑申请 SignPath Foundation 免费开源签名

### macOS (#34)

- Release 页面添加命令：`xattr -cr /Applications/OpenTypeless.app`
- 考虑申请 Apple Developer Program（$99/年）实现 notarization

### Linux deb 打包 (#39)

- 排查 CI 中 deb 打包流程是否正确生成有效 deb 文件
- 可能需要调整 `tauri.conf.json` 中 Linux 打包配置

### Linux NVIDIA EGL 崩溃 (#36)

- Rust 端启动时检测 NVIDIA 驱动 + Wayland 环境
- 检测到时自动设置 `WEBKIT_DISABLE_DMABUF_RENDERER=1`
- 在 Tauri WebView 初始化之前执行

### 改动文件

- `src-tauri/src/lib.rs`：添加 NVIDIA/Wayland 检测
- `src-tauri/tauri.conf.json`：可能调整 Linux 打包配置
- `README.md`：各平台安装说明
- `.github/workflows/`：可能调整 CI 打包流程

---

## 阶段 2：流水线可靠性

### 2.1 重试机制

**STT 请求重试：**
- 指数退避，最多 3 次（1s → 2s → 4s）
- 仅对可重试错误重试：网络超时、5xx、连接重置
- 不重试：4xx 客户端错误、认证失败
- 每次重试通过 Tauri event 通知前端

**LLM 请求重试：**
- 同样指数退避策略
- SSE 流已开始后不重试
- 仅在连接阶段或首次响应超时时重试

**实现位置：** 各 STT/LLM provider 内部，trait 接口不变。

### 2.2 共享 HTTP Client

- `reqwest::Client` 通过 Tauri state 管理，`app.manage()` 注入
- Pipeline 和测试/benchmark 命令共用同一 Client
- 连接池参数：`pool_max_idle_per_host(2)`、`pool_idle_timeout(30s)`
- 全局超时：30s

### 2.3 输出失败自动回退

回退链：**键盘输出 → 剪贴板输出 → 保存历史记录**

- 键盘模拟失败时自动回退到剪贴板模式
- 回退成功后通知前端："已复制到剪贴板，请手动粘贴"
- 剪贴板也失败时保存到历史记录并通知用户

### 2.4 超时配置

- STT 连接超时：10 秒
- LLM 连接超时：15 秒
- 超时后触发重试（如果还有重试次数）
- 暂不在 Settings UI 暴露，硬编码默认值

### 2.5 Linux 键盘输出检测 (#32)

- 启动时检测 X11 还是 Wayland
- Wayland 下自动切换剪贴板模式 + 一次性提示
- X11 下检测 xdotool，未安装则提示安装命令
- 检测逻辑在 `output/keyboard.rs` 初始化阶段

### 2.6 错误提示友好化（i18n）

Rust 端传结构化错误码，前端根据 code + locale 渲染。

```rust
struct UserError {
    code: &'static str,       // e.g. "stt_invalid_key"
    provider: Option<String>,
    retry_count: u32,
}
```

错误码映射：

| code | en | zh |
|------|----|----|
| `stt_invalid_key` | Invalid API Key, please check settings | API Key 无效，请检查设置 |
| `stt_timeout` | Connection timeout, retrying... | 网络连接超时，正在重试... |
| `stt_failed` | Speech recognition failed, please check your network | 语音识别失败，请检查网络连接 |
| `llm_failed` | Text polish failed, raw transcription saved | 文本润色失败，原始转录已保存 |
| `output_fallback_clipboard` | Copied to clipboard, paste manually | 已复制到剪贴板，请手动粘贴 |
| `output_wayland_unsupported` | Keyboard output unsupported on Wayland, switched to clipboard mode | Wayland 不支持键盘输出，已切换为剪贴板模式 |

### 改动文件

- `src-tauri/src/pipeline.rs`：集成重试、超时、回退
- `src-tauri/src/stt/*.rs`：各 provider 内加重试
- `src-tauri/src/llm/*.rs`：LLM provider 加重试
- `src-tauri/src/output/*.rs`：回退链 + Linux 检测
- `src-tauri/src/lib.rs`：共享 reqwest::Client 注入
- 新增 `src-tauri/src/error.rs`：统一错误类型
- `src/components/Capsule/*.tsx`：重试/回退状态 UI
- `src/i18n/locales/en.json` + `zh.json`：新增 errors 段
- `src/hooks/useTauriEvents.ts`：错误事件 i18n 渲染

---

## 阶段 3：代码架构优化

### 3.1 拆分 lib.rs（1339 行 → ~150 行 + 7 个模块）

```
src-tauri/src/
  lib.rs              -- 仅 run() 入口 + Tauri setup (~150 行)
  commands/
    mod.rs            -- 模块导出
    stt.rs            -- test_stt_connection, bench_stt_connection (~120 行)
    llm.rs            -- test_llm_connection, bench_llm_connection (~120 行)
    config.rs         -- get_config, save_config, reset_config 等 (~100 行)
    history.rs        -- 历史记录相关命令 (~80 行)
    dictionary.rs     -- 词典相关命令 (~60 行)
    misc.rs           -- get_version, get_foreground_app 等 (~50 行)
  tray.rs             -- 系统托盘菜单构建 + 事件处理 (~150 行)
  hotkey.rs           -- 热键解析 + 注册/注销 (~120 行)
```

不改变任何 Tauri 命令签名和外部行为。

### 3.2 拆分 pipeline.rs 的 stop()（320+ 行 → 3 个方法）

```rust
impl Pipeline {
    pub async fn stop(&mut self) -> Result<PipelineResult> {
        let transcript = self.transcribe().await?;
        let polished = self.polish(&transcript).await?;
        self.output_and_save(&polished).await?;
        Ok(result)
    }

    async fn transcribe(&mut self) -> Result<String> { ... }
    async fn polish(&mut self, text: &str) -> Result<String> { ... }
    async fn output_and_save(&mut self, text: &str) -> Result<()> { ... }
}
```

### 3.3 提取 STT Provider 配置常量

```rust
// src-tauri/src/stt/config.rs
pub struct SttProviderConfig {
    pub endpoint: &'static str,
    pub model: &'static str,
    pub extra_fields: Option<HashMap<&'static str, &'static str>>,
}

pub fn get_provider_config(provider: &SttProviderType) -> SttProviderConfig { ... }
```

消除 3 处重复（test_stt、bench_stt、stt/mod.rs）。

### 3.4 统一错误类型

```rust
// src-tauri/src/error.rs
pub struct UserError { code, message, details }

pub enum AppError {
    Network(reqwest::Error),
    Timeout(Duration),
    Api { status: u16, body: String },
    Output(String),
    Config(String),
}
```

`SttProvider` / `LlmProvider` trait 返回类型从 `anyhow::Result` 改为 `Result<T, AppError>`。

### 3.5 测试补充

- `pipeline.rs`：状态转换测试
- `output/keyboard.rs`：回退链测试
- `error.rs`：错误分类测试（可重试 vs 不可重试）
- `stt/config.rs`：provider 配置正确性测试

### 改动文件

- 新增 7-8 个文件（commands/*、tray.rs、hotkey.rs、error.rs、stt/config.rs）
- `lib.rs`：1339 行 → ~150 行
- `pipeline.rs`：stop() 拆分，总行数不变
- `stt/mod.rs` + `lib.rs`：消除 provider 配置重复
- 新增测试文件

---

## 不变的部分

- 所有 Tauri 命令签名
- 前端组件逻辑（除 i18n 和 capsule 状态提示外）
- trait 接口定义
