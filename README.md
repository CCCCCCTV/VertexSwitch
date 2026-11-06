# VertexSwitch

中文 | [English](README.en.md)

桌面上的 Vertex AI 多协议代理，带图形界面（Rust + Tauri）。

- 本地提供 Anthropic Messages、Gemini generateContent、OpenAI Chat Completions 三种接口，统一转发到 Google Cloud Vertex AI
- 服务账号认证与 token 自动刷新
- 托盘常驻、可视化配置模型、服务档位（Standard / Flex / Priority）、日志与用量
- 以 Windows 10/11 为主，macOS / Linux 顺带支持

依据 Google Cloud Vertex AI 官方文档从零实现。非 Google 官方项目。

## 状态

已发布，安装包在 [Releases](https://github.com/CCCCCCTV/VertexSwitch/releases/latest)，仍在持续更新。行为规格见 `docs/SPEC.md`，开发背景与里程碑见 `docs/HANDOFF.md`。

代理核心（`crates/vertexswitch-core`）已实现：

- Anthropic Messages：`/v1/messages`（流式 / 非流式）、`/v1/messages/count_tokens` → Vertex 上的 Claude
- Gemini 原生：`/v1beta/models/{model}:generateContent | streamGenerateContent | countTokens`
- Gemini 服务档位：Standard / Flex / Priority，并用 `usageMetadata.trafficType` 核对实际档位
- OpenAI Chat Completions：Gemini 兼容接口、MaaS 开放模型、OpenAI→Claude 转换（含工具调用与流式）
- Claude 格式调用 Gemini：`/v1/messages` 收到 Gemini 模型时翻译成 Gemini 原生请求（含工具调用、思考、流式、计数），Claude Code 等客户端可以直接用 Gemini 模型
- 模型列表：`/v1/models`（Anthropic / OpenAI 格式）、`/v1beta/models`（Gemini 格式）
- 管理接口 `/admin/api/*`：状态、配置读写（立即生效）、模型表、日志与请求记录（含 SSE 实时流）、指标、测试请求；配置与模型表文件热加载
- 上游繁忙（429 资源耗尽 / 限流、503、Claude 的 529 过载）时自动重试：默认最多 3 次、依次约等 1、2、4 秒，带随机抖动、遵守 `Retry-After`；可在“设置 → 代理服务”关掉、改次数，等待时间可以写成数字或关于第几次重试 `n` 的算式（如 `5`、`min(30, 3*n)`）；客户端断开即停止
- `/metrics`：Prometheus 指标
- 桌面程序（`src-tauri` + `ui`）：托盘常驻（关窗口即隐藏，托盘菜单可切换 Gemini 档位）、
  概览 / 请求 / 日志 / 模型 / 设置五个页面（中文 / English），代理核心在同一进程内运行
- 安装包（当前用户安装，不需要管理员）、单实例、开机自启（只在托盘运行）

## 安装

从 [GitHub Releases](https://github.com/CCCCCCTV/VertexSwitch/releases/latest) 下载 `VertexSwitch_x.y.z_x64-setup.exe` 运行即可，安装到当前用户目录，不需要管理员权限。
卸载时保留配置目录 `%APPDATA%\VertexSwitch`。

每个 Release 附有 `SHA256SUMS.txt`，发布说明里有可直接粘贴的 PowerShell 核对命令（输出 True 即下载的文件完整）。

首次启动后在“设置”里选择服务账号密钥文件即可使用。

程序启动后和每天会查一次 GitHub 上有没有新版本（只发送程序名和版本号，可在“设置 → 系统”里关掉）。日志同时写在 `%APPDATA%\VertexSwitch\logs` 里。

## 接入客户端

代理默认监听 `http://127.0.0.1:8787`（端口可在“设置”里改）。“概览”页的“接入地址”卡片列出了下面这些地址，点一下就能复制：

| 协议 | 环境变量 | 地址 | 谁会读 |
| --- | --- | --- | --- |
| Anthropic | `ANTHROPIC_BASE_URL` | `http://127.0.0.1:8787` | Claude Code、Anthropic SDK |
| OpenAI 兼容 | `OPENAI_BASE_URL` | `http://127.0.0.1:8787/v1` | OpenAI SDK |
| Gemini | `GOOGLE_GEMINI_BASE_URL` | `http://127.0.0.1:8787` | Gemini CLI、JS SDK（Python SDK 要在代码里设 `http_options.base_url`） |

想让 Claude Code 用 Gemini 模型，除了 `ANTHROPIC_BASE_URL`，再把模型名设成 Gemini 的：`ANTHROPIC_MODEL` 是主模型，`ANTHROPIC_DEFAULT_HAIKU_MODEL` 是后台小任务用的模型，例如 `gemini-3.5-flash` 和 `gemini-3.5-flash-lite`。Claude 的联网搜索等服务端工具在 Gemini 上用不了，会被跳过。

“设置”里的 API key（`server.api_key`）留空时，代理不检查客户端的 key；客户端如果必须填 key，随便填一个即可。设置了 API key 后，客户端要把它作为 API key 发送（`Authorization: Bearer`、`x-api-key`、`x-goog-api-key` 或查询参数 `key` 都可以）。

## 从源码构建桌面程序

需要 Node.js 与 Rust（MSVC）工具链：

```powershell
npm install
npx tauri dev                 # 开发：界面热更新
npx tauri build --no-bundle   # 发布版：target\release\VertexSwitch.exe
npx tauri build               # 另外生成安装包：target\release\bundle\nsis
```

`VertexSwitch.exe --minimized` 启动时不显示窗口，只在托盘。只预览界面（示例数据，不连接核心）：`npm run dev`。

## 命令行运行代理核心

```powershell
cargo run --release -p vertexswitch-core                 # 端口按配置文件（默认 8787）
cargo run --release -p vertexswitch-core -- --port 8788  # 桌面程序同时在运行时换个端口
```

配置文件位于 `%APPDATA%\VertexSwitch\config.toml`（不存在时使用默认值），示例：

```toml
[google]
credentials_file = 'C:\path\to\service-account.json'
project_id = ""        # 留空则从密钥文件读取
region = "global"      # global / us / eu / 具体区域

[gemini]
region = ""            # 留空则沿用 google.region；Flex 只支持 global
tier = "standard"      # 默认档位：standard / flex / priority
shared_only = false    # true 时只走 PayGo，绕过预配吞吐量
flex_server_timeout = 1800
openai_native = "auto" # OpenAI 格式的 Gemini 请求需要档位时转成原生格式（always / never 可选）

[gemini.model_tiers]   # 按上游模型单独设档位，优先于默认档位（界面“模型”页也能改）
"gemini-3.5-flash" = "flex"

[server]
api_key = ""           # 非空时客户端须携带此 key

[retry]
busy = true            # 上游繁忙（429 / 503 / 529）时自动重试
busy_attempts = 3      # 最多重试几次（1–10）
busy_wait = "2^(n-1)"  # 第 n 次重试前等几秒：数字或算式，可用 + - * / ^、括号、min()、max()
```

模型表默认内置；要修改，把 `crates/vertexswitch-core/data/models.toml` 复制到
`%APPDATA%\VertexSwitch\models.toml` 后编辑。

档位对 Gemini 原生接口和 OpenAI 兼容接口（`/v1/chat/completions`）都生效：Vertex 的兼容接口不接受档位，
需要档位时代理会把 OpenAI 格式的请求转成 Gemini 原生格式发送、再把响应转回来。
单个请求可用请求头 `X-VertexSwitch-Tier: flex|priority|standard` 覆盖档位，
响应头 `X-VertexSwitch-Tier` 给出实际使用的档位。

## 管理接口

只接受本机连接，需要管理 token。token 存在 Windows 凭据管理器里，可在界面“设置 → 系统”复制，或用 `vertexswitch-core admin-token` 打印：

```powershell
# 先在界面“设置 → 系统”点“复制”（或用源码构建的 vertexswitch-core admin-token）
$h = @{ Authorization = "Bearer " + (Get-Clipboard) }
$api = "http://127.0.0.1:8787/admin/api"

Invoke-RestMethod "$api/status" -Headers $h
# 改档位：立即生效并写回配置文件
Invoke-RestMethod "$api/config" -Method Patch -Headers $h -ContentType "application/json" `
  -Body '{"gemini":{"tier":"flex"}}'
Invoke-RestMethod "$api/requests?limit=20" -Headers $h
# 实时日志（SSE）
curl.exe -N -H "Authorization: $($h.Authorization)" "$api/logs/stream"
```

全部接口见 `docs/SPEC.md` 第 12 节。

## 开发

```powershell
npm run check                 # 界面类型检查
npm run build                 # 构建界面（Tauri 外壳编译时内嵌）
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

测试全部使用本地假上游（wiremock 与原始 TCP 服务），不会调用真实的 Vertex AI。

## 许可证

[GNU AGPL-3.0](LICENSE)（仅 3.0 版，`AGPL-3.0-only`）。可以自由使用、修改和分发，包括商用；但分发修改过的版本，或把修改过的版本作为网络服务提供给他人使用时，必须按同一许可证公开完整源代码。
