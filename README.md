# VertexSwitch

桌面上的 Vertex AI 多协议代理，带图形界面（Rust + Tauri）。

- 本地提供 Anthropic Messages、Gemini generateContent、OpenAI Chat Completions 三种接口，统一转发到 Google Cloud Vertex AI
- 服务账号认证与 token 自动刷新
- 托盘常驻、可视化配置模型、服务档位（Standard / Flex / Priority）、日志与用量
- 以 Windows 10/11 为主，macOS / Linux 顺带支持

依据 Google Cloud Vertex AI 官方文档从零实现。非 Google 官方项目。

## 状态

开发中（见 `docs/HANDOFF.md` 第 8 节里程碑）。行为规格见 `docs/SPEC.md`。

代理核心（`crates/vertexswitch-core`）已实现：

- Anthropic Messages：`/v1/messages`（流式 / 非流式）、`/v1/messages/count_tokens` → Vertex 上的 Claude
- Gemini 原生：`/v1beta/models/{model}:generateContent | streamGenerateContent | countTokens`
- Gemini 服务档位：Standard / Flex / Priority，并用 `usageMetadata.trafficType` 核对实际档位
- OpenAI Chat Completions：Gemini 兼容接口、MaaS 开放模型、本地 Ollama、OpenAI→Claude 转换（含工具调用与流式）
- 模型列表：`/v1/models`（Anthropic / OpenAI 格式）、`/v1beta/models`（Gemini 格式）
- 管理接口 `/admin/api/*`：状态、配置读写（立即生效）、模型表、日志与请求记录（含 SSE 实时流）、指标、测试请求；配置与模型表文件热加载
- `/metrics`：Prometheus 指标
- 桌面程序（`src-tauri` + `ui`）：托盘常驻（关窗口即隐藏，托盘菜单可切换 Gemini 档位）、
  概览 / 请求 / 日志 / 模型 / 设置五个页面，代理核心在同一进程内运行

- 安装包（当前用户安装，不需要管理员）、单实例、开机自启（只在托盘运行）

## 安装

从 GitHub Release 下载 `VertexSwitch_x.y.z_x64-setup.exe` 运行即可，安装到当前用户目录，不需要管理员权限。
安装包暂未签名，首次运行时 SmartScreen 可能提示，选择“仍要运行”。卸载时保留配置目录 `%APPDATA%\VertexSwitch`。

首次启动后在“设置”里选择服务账号密钥文件即可使用。

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
cargo run --release -p vertexswitch-core -- --port 8788
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

[ollama]
enabled = false
base_url = "http://127.0.0.1:11434"

[server]
api_key = ""           # 非空时客户端须携带此 key
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
$api = "http://127.0.0.1:8788/admin/api"

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
