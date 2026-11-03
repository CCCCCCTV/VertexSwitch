# VertexSwitch 交接报告

> 写于 2026-11-03。给接手的 Claude Code 会话和开发者看：读完这份就能开工，不需要之前的聊天记录。

## 1. 一句话

VertexSwitch 是一个**以 Windows 10/11 为主的桌面程序**（Rust + Tauri，macOS / Linux 可顺带支持）：在本机提供 Anthropic / Gemini / OpenAI 三种 API，用 Google Cloud 服务账号把请求转发到 Vertex AI，带托盘和图形界面。**依据官方文档从零实现**，不复用旧代码。

## 2. 背景与决定

| 项 | 内容 |
| --- | --- |
| 旧项目 | `vertex-proxy`（开源项目 prasadus92/vertex-proxy 0.4.0，本地又改了好几代），装在 `C:\Users\Admin\AppData\Local\Python\pythoncore-3.14-64\Lib\site-packages\vertex_proxy`，监听 `127.0.0.1:8787` |
| 旧项目状态 | **正在运行，不要停、不要改它的文件**。新项目开发期间用别的端口（建议 8788）并行 |
| 为什么重写 | main.py 1,352 行包办一切、多处重复、几处路由边界缺陷；用户要加 GUI；不想背旧代码的包袱 |
| 重写原则 | 从 Google / Anthropic 官方文档出发写代码，**不要把旧代码复制进本仓库**。旧代码只用来了解"需要哪些行为" |
| 平台 | 以 Windows 为主。改用 Tauri 后 macOS / Linux 可顺带支持；Android 不作为目标 |
| 主要用法 | 用户日常主要用 **Claude 走 `/v1/messages`**（Claude Code 等 Anthropic 客户端）。这条路径优先级最高 |
| 旧项目分析报告 | https://claude.ai/artifact/CCmhVoUzyeAWb3ifYSEceP （14 条缺陷 + 结构问题） |
| 旧代码快照 | 需要对照行为时，读上面的 site-packages 路径（只读），或用户手里的 `vertex_proxy-0.4.0-snapshot-20261103.zip` |

## 3. 环境

- 仓库：`D:\Admin\Documents\VertexSwitch`，分支 `main`，首个提交 `53d0399`（只有 README、.gitignore、.gitattributes）
- Git 身份已配置：`CCCCCCTV <2091449962@qq.com>`
- 机器上有 Python 3.14（pythoncore-3.14-64）、scoop、Node/bun。建议项目用独立虚拟环境（`uv` 或 `python -m venv .venv`），**不要装到全局 site-packages**
- `.gitignore` 已忽略 `.env`、`*service-account*.json`、`*credentials*.json`、`*-key.json`。服务账号密钥绝不入库
- 提交时 Git 提示 LF→CRLF 是正常现象（`.gitattributes` 已为 .py/.md/.toml 等固定 LF）

## 4. 功能需求

以下行为来自旧项目，是新项目要覆盖的功能清单。**每一条实现前都要对照第 6 节的官方文档核实**，旧项目的写法不等于正确。

### 4.1 对外接口（入口）

| 协议 | 路径 | 说明 |
| --- | --- | --- |
| Anthropic Messages | `POST /v1/messages`，`/anthropic/v1/messages` | **最高优先级**。流式与非流式 |
| Gemini 原生 | `POST /v1beta/models/{model}:{action}`，`/gemini/v1beta/models/...` | action = `generateContent` / `streamGenerateContent`，透传查询参数（如 `alt=sse`） |
| OpenAI Chat Completions | `POST /v1/chat/completions`，`/chat/completions`，`/openai/v1/chat/completions` 等 | 按 `model` 分流到 Claude / Gemini / MaaS / Ollama |
| 模型列表 | `GET /v1/models`，`/anthropic/v1/models`，`/gemini/v1beta/models`，`GET /v1/models/{id}` | 客户端探测用；旧项目还挂了很多前缀别名（`/models`、`/openai/v1/models`、`/api/v1/models`…），新项目用一张路由表统一注册 |
| 健康检查 | `GET /health` | 顺带验证 Google token 能否获取 |
| 指标 | `GET /metrics` | Prometheus 格式，可选 |

可选的代理自身鉴权：配置了 API key 时，除 `/health` 外都要 `Authorization: Bearer <key>`（旧项目这里有两处没鉴权，见 5 节）。

### 4.2 上游（Vertex AI）

主机名：区域为 `global` 时用 `aiplatform.googleapis.com`，否则 `{region}-aiplatform.googleapis.com`。

| 后端 | 上游地址 | 要点 |
| --- | --- | --- |
| Claude | `/v1/projects/{p}/locations/{r}/publishers/anthropic/models/{model}:rawPredict`（流式 `:streamRawPredict`） | 请求体去掉 `model`，加 `"anthropic_version": "vertex-2023-10-16"`；旧项目还去掉了 Claude Code 发来的 `context_management` 字段（核实 Vertex 当前是否仍不支持） |
| Gemini 原生 | `/v1/projects/{p}/locations/{r}/publishers/google/models/{model}:generateContent` 等 | 唯一可以带服务档位头的通道 |
| Gemini（OpenAI 兼容） | `/v1beta1/projects/{p}/locations/{r}/endpoints/openapi/chat/completions`，`model` 写成 `google/{model}` | **不能带 Flex/Priority 头**，旧项目实测会 400 "Flex API is not supported with chatCompletions API" |
| MaaS 开放模型（Kimi、GLM、Qwen、MiniMax、DeepSeek…） | 官方文档用的是同一个 `.../endpoints/openapi/chat/completions`，`model` 为 `{publisher}/{model}-maas` 形式 | **注意**：旧项目用的是 `.../{publishers/x/models/y}/chat/completions` 路径，与官方文档不一致，以官方文档为准并实测 |
| Ollama（本地） | `{base_url}/v1/chat/completions`；`GET {base_url}/api/tags` 自动发现模型 | **不需要 Google token**（旧项目缺陷 3） |

认证：服务账号 JSON → OAuth access token（有效期 60 分钟），scope `https://www.googleapis.com/auth/cloud-platform`。后台每约 50 分钟刷新；启动时刷新失败不能让程序退出；并发刷新要加锁。项目 ID 可从密钥文件读取。

### 4.3 服务档位（仅 Gemini 原生通道）

| 档位 | 请求头 |
| --- | --- |
| Standard（默认） | 不发 |
| 只用 PayGo（绕过预配吞吐量） | `X-Vertex-AI-LLM-Request-Type: shared` |
| Flex | `X-Vertex-AI-LLM-Shared-Request-Type: flex`，可配合上一行 |
| Priority | `X-Vertex-AI-LLM-Shared-Request-Type: priority`，可配合上一行 |
| 排队超时 | `X-Server-Timeout: <秒>`；旧项目 Flex 默认 1800（上限 30 分钟），本地读超时不得小于它 |

- 旧项目认为 Flex/Priority 只支持 `global` 区域，非 global 时自动降级为 Standard 并告警（核实）
- 某些模型固定走 Standard（旧项目默认 `gemini-4-argon`），对它连 `shared` 头也不发
- 用响应里 `usageMetadata.trafficType` 核对实际档位（`ON_DEMAND` / `ON_DEMAND_FLEX` / `ON_DEMAND_PRIORITY` / `PROVISIONED_THROUGHPUT`），请求 Flex 却没按 Flex 计费时告警；流式每个 chunk 都会带，同一请求只报一次

### 4.4 稳健性（旧项目踩过的坑，必须保留）

- 上游连接在**收到任何响应字节之前**断开（RemoteProtocolError、ConnectError、ReadError、WriteError、PoolTimeout 等）可以安全重试，旧项目重试 2 次、间隔 0.4s × 次数
- **配额耗尽特征**：Vertex 挂住约 20 秒以上、然后不发任何响应就断开 = 配额耗尽（本应是 429 RESOURCE_EXHAUSTED）。此时不重试，直接返回带说明的 504
- 非流式读超时默认 600 秒（大请求非流式可能要几分钟）；流式不能用短的读超时，但应设一个很长的空闲上限，避免永久挂起
- 504 表示"等上游超时"，502 表示"上游连接/响应坏了"
- 任何异常都要返回 JSON，不能让连接无响应断开（客户端会报 "Server disconnected without sending a response"）
- 流式响应开始后出错：发一个错误事件再结束流，格式按入口协议（Anthropic 用 `event: error`，OpenAI 用 `data: {"error":...}`）
- 服务端 keep-alive 至少 75 秒（uvicorn 默认 5 秒会和客户端连接池竞争）；可选每个响应带 `Connection: close`

### 4.5 模型别名（旧项目默认表）

以下是旧项目里写的模型 ID，**必须到 Vertex Model Garden 核实**后放进可编辑的配置文件（不要写死在代码里）。

- Claude：`claude-opus-4-8`、`claude-opus-4-7`、`claude-opus-4-6`、`claude-sonnet-4-6`（新一代 ID 不带日期，带 `@日期` 会 404）；`claude-sonnet-4-5` → `claude-sonnet-4-5@20250929`；`claude-opus-4-5` → `claude-opus-4-5@20251101`；`claude-haiku-4-5` → `claude-haiku-4-5@20251001`（同时接受带日期的别名，如 `claude-sonnet-4-5-20250929`）
- Gemini：`gemini-4-argon`、`gemini-3.8-flash`、`gemini-3.7-flash`、`gemini-3.6-flash`、`gemini-3.5-flash`、`gemini-3.5-flash-lite`、`gemini-3.1-pro` → `gemini-3.1-pro-preview`、`gemini-3-pro` → `gemini-3.1-pro-preview`、`gemini-3.1-flash-lite`、`gemini-3-flash` → `gemini-3-flash-preview`、`gemini-2.5-pro`、`gemini-2.5-flash`、`gemini-2.5-flash-lite`、`gemini-2.0-flash` → `gemini-2.0-flash-001`
- MaaS：Kimi（k2.5、k2）、GLM（5、5.1、4.6）、MiniMax（m2.5、m1）、Qwen（3.5、3）、Grok（4.20、4.1-fast）

## 5. 旧项目的缺陷，新项目要避免

| 旧缺陷 | 新项目的对策 |
| --- | --- |
| OpenAI→Claude 转换丢掉 `tools`/`tool_choice`，图片块原样透传，流式工具调用不翻译 | 转换层完整支持工具、图片、流式工具调用（有状态累积 `input_json_delta`），写成纯函数并充分测试 |
| 分级头在路由判断前就加上，会发到不支持的端点 | 由各后端声明 `supports_tier`，先路由后加头 |
| 走 Ollama 也要先取 Google token | 后端声明 `needs_google_token` |
| OpenAI 兼容 Gemini 不发分级头却按分级核对，误报告警 | 核对只在真的发了分级头时进行 |
| 错误体是 FastAPI 默认 `{"detail":...}` | 按入口协议输出：Anthropic `{"type":"error","error":{"type":...,"message":...}}`，OpenAI `{"error":{...}}`，Gemini `{"error":{"code","message","status"}}` |
| 流式请求在上游返回前就记为 200，流式 token 不统计 | 流结束时按真实状态和 usage 记录 |
| 路由重复注册、部分探测接口和 /metrics 不鉴权、API key 非常数时间比较 | 一张路由表；统一依赖；`secrets.compare_digest` |
| 模块级全局状态、`.env` 从当前目录读 | 状态挂在 app 上；配置固定在 `%APPDATA%\VertexSwitch\` |
| 模型表写死在代码里、Ollama 只在启动时发现 | 配置文件 + 热加载；定时或手动刷新 |

## 6. 官方文档入口

- Claude on Vertex（Anthropic）：https://platform.claude.com/docs/en/build-with-claude/claude-on-vertex-ai
- Claude on Vertex（Google）：https://docs.cloud.google.com/vertex-ai/generative-ai/docs/partner-models/claude/use-claude
- Gemini 的 OpenAI 兼容接口：https://docs.cloud.google.com/vertex-ai/generative-ai/docs/multimodal/call-gemini-using-openai-library
- MaaS 开放模型调用：https://docs.cloud.google.com/vertex-ai/generative-ai/docs/maas/call-open-model-apis
- MaaS 模型总览：https://docs.cloud.google.com/vertex-ai/generative-ai/docs/maas/overview
- Flex PayGo：https://docs.cloud.google.com/vertex-ai/generative-ai/docs/flex-paygo
- Priority PayGo：https://docs.cloud.google.com/vertex-ai/generative-ai/docs/priority-paygo
- Gemini generateContent REST 参考、服务账号认证、Anthropic Messages API 流式事件格式：开工时在官方站点检索最新页面

## 7. 目标架构

> **2026-11-03 变更**：技术栈由 Python（FastAPI + pywebview）改为 **Rust + Tauri 2 + TypeScript**。
> 原因：后台常驻一个 Python 进程不方便（Alt+Tab 常显、打包 40–60 MB、启动慢、易被杀毒误报）；
> Tauri 单个 exe 约 10 MB，内存小，且 Windows / macOS / Linux 桌面可同一套代码（Android 不作为目标）。
> Python 原型（M0–M3）保留在仓库里作为参考实现，Rust 版达到同等功能后删除。
> 行为规格见 `docs/SPEC.md`；Python 原型的 `tests/` 是它的可执行版本。

### 7.1 进程结构

```text
VertexSwitch.exe（Tauri 2，单进程，Windows 下无控制台窗口）
   ├── 托盘图标、单实例、开机自启（Tauri 插件）
   ├── 主窗口（WebView2 显示 TypeScript 前端；关窗口=隐藏到托盘，不退出）
   └── 代理核心（同进程内的 tokio 任务，axum 服务）
          ├── 业务路由：/v1/messages、/v1beta/models/...、/v1/chat/completions ...
          └── 管理：Tauri command 直接调用核心；另提供 /admin/api/*（只绑 127.0.0.1，独立 token）供 curl
vertexswitch-core.exe（同一核心库的命令行版，无界面运行，便于服务器/调试）
```

- 核心与界面同进程：界面（WebView）崩溃不影响 Rust 主进程；改端口时只重启 axum 服务任务
- 配置与日志在 `%APPDATA%\VertexSwitch\`（其他平台用对应的配置目录）；密钥路径和管理 token 存系统凭据库（`keyring` crate）
- 打包：Tauri bundler（Windows 用 NSIS 安装包）

### 7.2 代码结构

```text
Cargo.toml                    # workspace
crates/vertexswitch-core/     # 核心库 + 命令行 bin
  src/config.rs               # 配置（TOML）
  src/auth.rs                 # 服务账号 JWT(RS256) → access token，加锁刷新
  src/transport.rs            # 上游请求：超时、重试、配额识别
  src/errors.rs               # 按入口协议渲染错误
  src/registry.rs             # 模型表与路由（models.yaml）
  src/tiers.rs                # 服务档位与 trafficType 核对（纯函数）
  src/backends/               # vertex.rs、claude.rs、gemini.rs、openai_compat.rs
  src/frontends/              # anthropic.rs、gemini.rs、openai.rs、models.rs、health
  src/translate/              # openai_to_anthropic.rs（纯函数 + 流式状态机）
  tests/                      # 用 wiremock 起本地假上游，不连真实 Vertex
src-tauri/                    # Tauri 外壳：托盘、窗口、command、单实例、自启
ui/                           # TypeScript + Vite 前端
src/vertexswitch/ , tests/    # Python 原型（参考实现，达到同等功能后删除）
```

主要依赖：tokio、axum、reqwest（rustls、stream）、serde / serde_json / serde_yaml / toml、jsonwebtoken（RS256）、tracing、wiremock（测试）、tauri 2 及其 tray / single-instance / autostart 插件。
测试时通过配置里隐藏的上游地址覆盖项把 Vertex / OAuth 指向 wiremock。

## 8. 里程碑

| # | 内容 | 完成标准 |
| --- | --- | --- |
| M0–M3 | Python 原型（已完成，作为参考） | 125 个 Mock 测试；Gemini 原生与档位已实测 |
| R0 ✅ | Rust 工具链、Cargo workspace、fmt / clippy / test | `cargo test` 跑通 |
| R1 ✅ | 核心基础 + Gemini 原生 + 档位（用户密钥只能调 Gemini，先做这条） | SPEC §1–7、§8.2、§9 的测试移植通过；真实验收同 Python 版 |
| R2 | Anthropic `/v1/messages`、count_tokens | SPEC §8.1 测试移植通过 |
| R3 | OpenAI 入口与 OpenAI→Claude 转换、模型列表、Ollama | SPEC §8.3–8.4、§10 测试移植通过；删除 Python 原型 |
| R4 | 管理接口、日志流、指标、模型表热加载 | 用 curl 能完成所有管理操作 |
| R5 | Tauri 外壳与界面 | 托盘启动、改配置、看日志、切档位 |
| R6 | 安装包、开机自启、单实例 | 干净 Windows 上安装即用 |
| R7 | 迁移 | 客户端逐个切到 VertexSwitch，确认稳定后再停旧 vertex-proxy |

## 9. 工作约定

- 不停、不改正在运行的旧 vertex-proxy（8787）；新程序开发期间用 8788
- 不复制旧代码；行为以官方文档为准，本报告第 4 节是需求清单不是实现
- 密钥、`.env`、真实请求日志不入库
- 测试默认用假上游（Python: MockTransport；Rust: wiremock），不消耗 Vertex 配额；需要真实调用时先告诉用户
- 用户的服务账号目前只能调用 Gemini；Claude 路径只能用假上游测试
- 小步提交，提交信息说明"做了什么、为什么"
