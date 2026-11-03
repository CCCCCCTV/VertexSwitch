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

管理接口、图形界面、安装包尚在开发。

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
tier = "standard"      # standard / flex / priority
shared_only = false    # true 时只走 PayGo，绕过预配吞吐量
flex_server_timeout = 1800

[ollama]
enabled = false
base_url = "http://127.0.0.1:11434"

[server]
api_key = ""           # 非空时客户端须携带此 key
```

模型表默认内置；要修改，把 `crates/vertexswitch-core/data/models.toml` 复制到
`%APPDATA%\VertexSwitch\models.toml` 后编辑。

单个请求可用请求头 `X-VertexSwitch-Tier: flex|priority|standard` 覆盖档位，
响应头 `X-VertexSwitch-Tier` 给出实际使用的档位。

## 开发

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

测试全部使用本地假上游（wiremock 与原始 TCP 服务），不会调用真实的 Vertex AI。
