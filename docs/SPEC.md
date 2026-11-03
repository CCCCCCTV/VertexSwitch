# VertexSwitch 行为规格

> 2026-11-03 从 Python 原型（M0–M3，提交 `e7cabbc`）整理而来，供 Rust 实现对照。
> 每一条都已对照官方文档核实；标"实测"的已用真实密钥在 Vertex 上验证过。
> Python 原型的测试用例（`tests/`，125 个）就是这份规格的可执行版本，Rust 版要逐条移植。

## 1. 配置

位置：`%APPDATA%\VertexSwitch\config.toml`；环境变量 `VERTEXSWITCH_HOME` 可改目录（开发/测试用）。
文件不存在时用默认值，不自动写盘。不从当前目录读 `.env`。

| 段 | 键 | 默认 | 说明 |
| --- | --- | --- | --- |
| server | host / port | `127.0.0.1` / `8788` | 开发期用 8788（旧代理占用 8787） |
| server | api_key | `""` | 非空时除 `/health` 外都要鉴权 |
| server | keep_alive_seconds | 75 | 服务端 keep-alive，避免和客户端连接池竞争（Python/uvicorn 需要；Rust 版 hyper 默认不主动关闭空闲连接，该项暂不使用） |
| google | credentials_file | `""` | 为空时退回 `GOOGLE_APPLICATION_CREDENTIALS` |
| google | project_id | `""` | 为空时从密钥文件读 |
| google | region | `global` | Claude 默认区域，也是其他后端的兜底区域 |
| google | token_refresh_minutes | 50 | |
| timeouts | connect / read / stream_idle / write / pool | 10 / 600 / 900 / 60 / 10 秒 | read=非流式整体等待；stream_idle=流式两段数据间最长空闲 |
| retry | attempts / backoff_seconds / quota_hang_seconds | 2 / 0.4 / 20 | 见 §4 |
| claude | anthropic_version | `vertex-2023-10-16` | |
| claude | drop_body_fields | `["context_management"]` | 转发前删掉的顶层字段；Vertex 是否仍拒绝 context_management **未核实** |
| gemini | region | `""` | 为空沿用 google.region（Flex 只支持 global，所以常需单独设） |
| gemini | tier | `standard` | standard / flex / priority |
| gemini | shared_only | false | 加 `X-Vertex-AI-LLM-Request-Type: shared` |
| gemini | flex_server_timeout | 1800 | 封顶 1800 |
| gemini | flex_unsupported_models | `["gemini-4-argon"]` | 官方注明不支持 Flex |
| gemini | openai_api_version | `v1beta1` | Gemini 的 OpenAI 兼容接口版本 |
| maas | region / api_version | `""` / `v1` | |
| ollama | enabled / base_url | false / `http://127.0.0.1:11434` | |
| openai | default_max_tokens | 8192 | OpenAI→Claude 时客户端没给 max_tokens 的默认值 |

## 2. 认证（服务账号 → access token）

- 依据：https://developers.google.com/identity/protocols/oauth2/service-account#httprest
- 用密钥文件里的 `private_key` 以 RS256 签 JWT（`iss`=client_email，`scope`=`https://www.googleapis.com/auth/cloud-platform`，`aud`=token_uri，`iat`/`exp` 相差 3600 秒），POST 到 `token_uri`，`grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer`。
- 剩余有效期少于 5 分钟就刷新；并发请求只让一个去刷新（加锁、双重检查）。
- 后台每 50 分钟刷新一次；启动时刷新失败只记日志，不退出。按需获取时失败 → 返回 500，信息包含原因（如"未配置服务账号密钥文件"）。
- 项目 ID：配置优先，否则用密钥文件里的 `project_id`。项目 ID 要在取过 token（读过密钥文件）之后再读。
- **不要读取或打印密钥内容**；日志里不能出现 token。

## 3. Vertex 主机与路径

`vertex_base_url(region)`（依据 Anthropic 官方 SDK 与 Google 文档；实测 global）：

| region | base |
| --- | --- |
| `global` | `https://aiplatform.googleapis.com/v1` |
| `us` / `eu`（多区域） | `https://aiplatform.{us,eu}.rep.googleapis.com/v1` |
| 其他（如 `us-east5`） | `https://{region}-aiplatform.googleapis.com/v1` |

publisher 模型：`{base}/projects/{p}/locations/{region}/publishers/{publisher}/models`

| 后端 | URL | 请求体改写 |
| --- | --- | --- |
| Claude | `.../publishers/anthropic/models/{m}:rawPredict`，流式 `:streamRawPredict` | 删 `model`；删 drop_body_fields；`anthropic_version` 缺省时补上 |
| Claude count_tokens | `.../publishers/anthropic/models/count-tokens:rawPredict` | 同上，但 `model` 写回请求体（上游 ID） |
| Gemini 原生 | `.../publishers/google/models/{m}:{action}`，action ∈ generateContent / streamGenerateContent / countTokens | `contents[]` 里缺 `role` 的补 `"user"`（实测 Vertex 要求有 role） |
| Gemini（OpenAI 兼容） | `{host}/v1beta1/projects/{p}/locations/{r}/endpoints/openapi/chat/completions` | `model` = `google/{m}` |
| MaaS | `{host}/v1/projects/{p}/locations/{r}/endpoints/openapi/chat/completions` | `model` 原样（`publisher/model`） |
| Ollama | `{base_url}/v1/chat/completions`；发现：`GET {base_url}/api/tags` | `model` 去掉 `ollama/` 前缀；**不取 Google token** |

转发的请求头：`Authorization: Bearer <token>`、`Content-Type: application/json`；Claude 额外透传客户端的 `anthropic-beta`。客户端的 `anthropic-version` **不**转发（版本在请求体里）。

## 4. 上游请求：超时、重试、配额识别

- 一律先拿到响应头（不读正文）再决定怎么处理。
- 超时：非流式 read=600；流式 read=stream_idle（900）；Flex 时 read 至少 `X-Server-Timeout + 60`。
- **只在收到任何响应字节之前**出错时重试：ConnectError、ConnectTimeout、PoolTimeout、RemoteProtocolError（对端断开）、ReadError、WriteError。最多重试 2 次，间隔 0.4 × 第几次。
- **配额耗尽特征**（实测旧项目经验）：从发出到"对端断开 / 读错误"超过 20 秒，且没有任何响应 → 不重试，返回 504 并说明"通常表示配额耗尽"。ConnectError 不算（连接都没建立）。上游不是 Vertex（Ollama）时不做这个判断。
- ReadTimeout（等响应头超时）→ 504，不重试。WriteTimeout → 504。
- 重试用尽 → 502 "无法连接 {上游名}（已重试 N 次）"。
- 读正文时出错：ReadTimeout → 504；其他 → 502。
- 状态码约定：504 = 等上游超时；502 = 上游连接/响应坏了。

## 5. 错误格式（按入口协议）

入口协议判定（用于兜底渲染）：路径以 `/anthropic/` 或 `/v1/messages` 开头 → Anthropic；以 `/gemini/`、`/v1beta/` 开头，或含 `/models/` 且最后一段带 `:` → Gemini；其他 → OpenAI。

| 协议 | 错误体 |
| --- | --- |
| Anthropic | `{"type":"error","error":{"type":T,"message":M}}`；T：400 invalid_request_error，401 authentication_error，402 billing_error，403 permission_error，404 not_found_error，413 request_too_large，429 rate_limit_error，500 api_error，504 timeout_error，529 overloaded_error；其他 4xx→invalid_request_error，5xx→api_error。上游已是 Anthropic 错误体时原样透传 |
| Gemini | `{"error":{"code":C,"message":M,"status":S}}`；S 用 google.rpc 名（400 INVALID_ARGUMENT，401 UNAUTHENTICATED，403 PERMISSION_DENIED，404 NOT_FOUND，429 RESOURCE_EXHAUSTED，500 INTERNAL，502/503 UNAVAILABLE，504 DEADLINE_EXCEEDED…）。上游 Google 错误体（可能包在单元素数组里）原样透传 |
| OpenAI | `{"error":{"message":M,"type":T,"param":null,"code":C}}`；T：400 invalid_request_error，401 authentication_error，403 permission_error，404 not_found_error，429 rate_limit_error，其他 api_error |

- 上游非 2xx：状态码原样；信息从 `error.message`（对象或单元素数组）/ `error`（字符串）/ `message` 中取；都没有就用正文前 2000 字符；正文为空用"上游返回 HTTP {status}"。
- 任何异常都返回 JSON，不能让连接无响应断开。未知路由 404 也按协议格式。
- **流开始后出错**：先发 `\n\n` 结束可能被截断的事件，再发错误事件——Anthropic：`event: error\ndata: {...}\n\n`；OpenAI/Gemini：`data: {...}\n\n`。然后结束流。

## 6. 代理鉴权

`server.api_key` 非空时，除 `/health` 外都要求以下任一项等于它（常数时间比较）：
`Authorization: Bearer <key>`、`x-api-key`、`x-goog-api-key`、查询参数 `key`。否则 401（按协议格式）。
查询参数 `key` 不转发给上游。

## 7. 模型表与路由

模型表 `models.toml`（Rust 版；Python 原型为 models.yaml）：包内默认 + `%APPDATA%\VertexSwitch\models.toml`（存在则整体优先）。结构：`[backend]` 段下 `"alias" = "upstream_id"`，backend ∈ claude / gemini / maas；保持声明顺序。

解析顺序（`resolve(name)`）：
1. 去掉首尾空白和 `models/` 前缀
2. `ollama/x` → Ollama `x`
3. 去掉 `google/`、`anthropic/` 前缀
4. Gemini 表命中 → 表值；否则 `gemini-*` → 原样
5. Claude 表命中 → 表值；否则 `claude-…-YYYYMMDD` → `claude-…@YYYYMMDD`；否则 `claude-*` → 原样
6. MaaS 表命中 → 表值；否则含 `/` → MaaS 原样
7. 在已发现的 Ollama 模型里 → Ollama
8. 否则未知（404）

默认表（2026-11-03 核实，Gemini 已用 countTokens 在 global 实测）：
- Claude：fable-5-1、fable-5、opus-5-5、opus-5、opus-4-8、opus-4-7、opus-4-6、sonnet-5-5、sonnet-5、sonnet-4-6、haiku-5-5 原样；opus-4-5→`@20251101`；sonnet-4-5→`@20250929`；haiku-4-5→`@20251001`
- Gemini：4-argon、3.8-flash、3.7-flash（2027-01-28 下线）、3.6-flash（**2026-11-19 下线**）、3.5-flash、3.5-flash-lite、3.1-flash-lite、3.1-pro-preview、3-flash-preview 原样；3.1-pro、3-pro → 3.1-pro-preview；3-flash → 3-flash-preview
- 已下线、不得出现在默认表：gemini-2.5-pro/flash/flash-lite、2.0-flash(-001)、3-pro-preview
- MaaS：空（官方未给出清单，核实后再加）

Ollama：启用时启动即调 `/api/tags` 发现（失败只告警），也应支持手动/定时刷新。

## 8. 入口

### 8.1 Anthropic Messages（最高优先级）

- `POST /v1/messages`、`/anthropic/v1/messages`；`POST .../v1/messages/count_tokens`
- 请求体必须是 JSON 对象（否则 400）；`model` 必填非空（否则 400）；解析结果不是 Claude → 404。
- 先取 token，再取项目 ID，再拼 URL。
- 上游非 2xx（含流式请求）→ 读完错误体，按 §5 返回 JSON。
- 非流式：正文与 content-type 原样返回。流式：`text/event-stream`，`Cache-Control: no-cache`，字节原样转发；中途断开按 §5 发 `event: error`。

### 8.2 Gemini 原生

- `POST /v1beta/models/{model}:{action}`、`/gemini/v1beta/models/...`、`/v1/models/{model}:{action}`；`{model}` 可带 `models/` 前缀。
- action 不在三者之内 / 模型不是 Gemini → 404（Gemini 格式）。
- 查询参数（如 `alt=sse`）原样转发，去掉 `key`。
- 生成类 action 按 §9 加档位头；countTokens 不加。
- 响应头 `X-VertexSwitch-Tier: <实际档位>`（countTokens 不带）。
- 请求头 `X-VertexSwitch-Tier` 可按请求覆盖配置档位；该头不转发给上游。
- 用响应（含流）里的 `trafficType` 核对档位，同一请求只告警一次。
- 流式 content-type 跟随上游（`alt=sse` 时为 event-stream，否则为 JSON 数组流）。实测两种都正常。

### 8.3 OpenAI Chat Completions

- `POST /v1/chat/completions`、`/chat/completions`、`/openai/v1/chat/completions`、`/api/v1/chat/completions`
- 按 `model` 分流：Gemini / MaaS / Ollama 原样转发（见 §3），流式转发时 `text/event-stream`，中途出错按 OpenAI 格式发错误；Claude 走 §10 转换。
- Ollama 未启用 → 400。Gemini 兼容接口**绝不能**带档位头（Vertex 返回 400 "Flex API is not supported with chatCompletions API"）。

### 8.4 模型列表

- `GET /v1/models`、`/anthropic/v1/models`：带 `anthropic-version` 头或 `/anthropic/` 前缀 → Anthropic 格式（只列 Claude）：`{"data":[{"type":"model","id","display_name","created_at":"1970-01-01T00:00:00Z"}],"has_more":false,"first_id","last_id"}`；否则 OpenAI 格式（Claude + Gemini + MaaS 别名 + 启用时 `ollama/<名字>`）：`{"object":"list","data":[{"id","object":"model","created":0,"owned_by":"vertexswitch"}]}`
- `GET /v1/models/{id}`：同上判定格式；未知 → 404
- `GET /v1beta/models`、`/gemini/v1beta/models`：`{"models":[{"name":"models/<id>","displayName","supportedGenerationMethods":["generateContent","streamGenerateContent","countTokens"]}]}`；`GET /v1beta/models/{id}` 单个，未知 → 404

### 8.5 健康检查

`GET /health`（不鉴权）：`{"status":"ok"|"degraded","version","region","gemini_region","gemini_tier","google_token":"ok"|<错误>,"project_id"}`；拿不到 token → 503。

## 9. 服务档位（仅 Gemini 原生生成请求）

依据：Flex PayGo、Priority PayGo 官方文档。**全部实测通过**（2026-11-03，global；Python 原型与 Rust R1 各测一次）。
Rust 实测时 Priority 请求一次被上游降级为 `ON_DEMAND`（文档所述的无空余容量情形），告警按设计触发。

- 头：`X-Vertex-AI-LLM-Shared-Request-Type: flex|priority`；`shared_only` 时再加 `X-Vertex-AI-LLM-Request-Type: shared`；Flex 加 `X-Server-Timeout: <秒>`（1..1800）。
- Flex：只支持 `global`；不支持 flex_unsupported_models 里的模型（gemini-4-argon）。
- Priority：支持 `global`、`us`、`eu`（旧项目误以为只支持 global）；argon 可用。
- 不支持 → 降级为 Standard、记告警，且**任何档位头都不发**（包括 shared）。
- 明确选 Standard 且 shared_only → 只发 `Request-Type: shared`。
- trafficType 核对（只在真的发了 Shared-Request-Type 头时）：flex 期望 `ON_DEMAND_FLEX`，priority 期望 `ON_DEMAND_PRIORITY`；未开 shared_only 时 `PROVISIONED_THROUGHPUT` 视为正常（先用预配吞吐量是文档行为）；其他值告警。Priority 降级为 `ON_DEMAND` 是文档描述的正常现象，但仍告警以便用户知道按什么计费。
- trafficType 从字节流里用正则 `"trafficType"\s*:\s*"([A-Z_]+)"` 找第一个，要能跨分块匹配。

## 10. OpenAI → Claude 转换

请求：
- system / developer 消息 → `system`（多条用空行连接）
- user：字符串 → text 块；数组：`text` → text，`image_url` 的 `data:<mime>;base64,<data>` → base64 image；**URL 图片 → 400**（Vertex 不支持 URL 来源），在请求上游之前就拒绝
- assistant：文本 + `tool_calls` → text 块 + `tool_use`（`arguments` JSON 解析失败 → 400）
- tool：→ user 消息里的 `tool_result`（tool_use_id = tool_call_id）
- 同角色相邻消息合并（多个 tool 结果与随后的 user 文本合到同一条 user 消息，tool_result 在前）
- `max_completion_tokens` / `max_tokens` / 默认值 → `max_tokens`；`temperature`、`top_p` 原样；`stop`（字符串或数组）→ `stop_sequences`；`user` → `metadata.user_id`；`stream`
- tools：只接受 `type=function` → `{name, description?, input_schema}`（parameters 缺省为空 object schema）；其他类型 → 400
- tool_choice：auto→auto，none→none，required→any，`{type:function,function:{name}}`→`{type:tool,name}`；`parallel_tool_calls:false` → 加 `disable_parallel_tool_use: true`（none 除外；没给 tool_choice 时按 auto）
- `n > 1` → 400；空 messages → 400；未知角色 → 400

非流式响应：text 拼接为 `content`（无文本为 null）；thinking → `reasoning_content`；tool_use → `tool_calls[{id,type:function,function:{name,arguments:JSON字符串}}]`；`model` 用客户端发来的名字；finish_reason：end_turn/stop_sequence/pause_turn→stop，max_tokens/model_context_window_exceeded→length，tool_use→tool_calls，refusal→content_filter，其他→stop；usage：prompt = input + cache_read + cache_creation，completion = output，cached_tokens = cache_read（>0 时给出 `prompt_tokens_details`）。

流式：
- SSE 解析：增量 UTF-8 解码（多字节字符可能被切开），每次对整个缓冲区把 CRLF 归一为 LF（CRLF 可能被切开）
- message_start → `{role:"assistant",content:""}`，记下 id 与 usage
- content_block_start(tool_use) → 分配递增的 tool 下标，发 `tool_calls[{index,id,type,function:{name,arguments:""}}]`
- text_delta → content；thinking_delta → reasoning_content；input_json_delta → `tool_calls[{index,function:{arguments:片段}}]`（按 content block 下标映射）
- message_delta → finish_reason 块，累计 usage
- message_stop → 结束；`stream_options.include_usage` 时追加 `choices:[]` + usage 的块；最后 `data: [DONE]`
- 上游 error 事件 → 转成 OpenAI 错误 data 后结束（不发 [DONE]）
- 上游没发 message_stop 就结束 → 发错误"流式响应提前结束"（不发 [DONE]）
- ping、未知事件忽略

## 11. 日志

- 命令行模式输出 UTF-8（实测：Windows 重定向时默认 ANSI 代码页导致中文乱码）
- 档位降级、trafficType 不符、配额耗尽、上游断流 → WARNING
- 不记录请求/响应正文（含真实对话）
