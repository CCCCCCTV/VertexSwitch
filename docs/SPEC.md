# VertexSwitch 行为规格

> 2026-11-03 从 Python 原型（M0–M3，提交 `e7cabbc`）整理而来，供 Rust 实现对照。
> 每一条都已对照官方文档核实；标"实测"的已用真实密钥在 Vertex 上验证过。
> 原型的 125 个测试已全部移植到 Rust（`crates/vertexswitch-core` 的单元测试与 `tests/`），它们是这份规格的可执行版本。

## 1. 配置

位置：`%APPDATA%\VertexSwitch\config.toml`；环境变量 `VERTEXSWITCH_HOME` 可改目录（开发/测试用）。
文件不存在时用默认值，不自动写盘。不从当前目录读 `.env`。

| 段 | 键 | 默认 | 说明 |
| --- | --- | --- | --- |
| server | host / port | `127.0.0.1` / `8787` | 0.1.1 及以前默认 8788；配置文件里写了端口的（在界面里保存过设置就会写上）不受影响 |
| server | api_key | `""` | 非空时除 `/health` 外都要鉴权 |
| server | keep_alive_seconds | 75 | 服务端 keep-alive，避免和客户端连接池竞争（Python/uvicorn 需要；Rust 版 hyper 默认不主动关闭空闲连接，该项暂不使用） |
| google | credentials_file | `""` | 为空时退回 `GOOGLE_APPLICATION_CREDENTIALS` |
| google | project_id | `""` | 为空时从密钥文件读 |
| google | region | `global` | Claude 默认区域，也是其他后端的兜底区域 |
| google | token_refresh_minutes | 50 | |
| timeouts | connect / read / stream_idle / write / pool | 10 / 600 / 900 / 60 / 10 秒 | read=非流式整体等待；stream_idle=流式两段数据间最长空闲 |
| retry | attempts / backoff_seconds / quota_hang_seconds | 2 / 0.4 / 20 | 连接错误重试，见 §4 |
| retry | busy / busy_attempts / busy_wait | `true` / 3 / `"2^(n-1)"` | 上游繁忙重试，见 §4；界面“设置 → 代理服务”可开关、改次数（1–10，核心按最多 10 次算）和等待算式 |
| claude | anthropic_version | `vertex-2023-10-16` | |
| claude | drop_body_fields | `["context_management"]` | 转发前删掉的顶层字段；Vertex 是否仍拒绝 context_management **未核实** |
| gemini | region | `""` | 为空沿用 google.region（Flex 只支持 global，所以常需单独设） |
| gemini | tier | `standard` | 默认档位：standard / flex / priority |
| gemini | model_tiers | `{}` | 按上游模型 ID 单独设的档位，如 `{"gemini-3.5-flash" = "flex"}`；优先于 tier |
| gemini | shared_only | false | 加 `X-Vertex-AI-LLM-Request-Type: shared` |
| gemini | flex_server_timeout | 1800 | 封顶 1800 |
| gemini | flex_unsupported_models | `["gemini-4-carbon", "gemini-4-argon"]` | 不支持 Flex 的模型；内置名单（Flex PayGo 文档支持列表里没有的）不管这里怎么写都按不支持处理，读配置时也并进这份名单 |
| gemini | openai_api_version | `v1beta1` | Gemini 的 OpenAI 兼容接口版本 |
| gemini | openai_native | `auto` | OpenAI 格式的 Gemini 请求：auto 需要档位头时转原生（§13）/ always 总是转 / never 总走兼容接口（档位不生效） |
| maas | region / api_version | `""` / `v1` | |
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

转发的请求头：`Authorization: Bearer <token>`、`Content-Type: application/json`；Claude 额外透传客户端的 `anthropic-beta`。客户端的 `anthropic-version` **不**转发（版本在请求体里）。

## 4. 上游请求：超时、重试、配额识别

- 一律先拿到响应头（不读正文）再决定怎么处理。
- 超时：非流式 read=600；流式 read=stream_idle（900）；Flex 时 read 至少 `X-Server-Timeout + 60`。
- **只在收到任何响应字节之前**出错时重试：ConnectError、ConnectTimeout、PoolTimeout、RemoteProtocolError（对端断开）、ReadError、WriteError。最多重试 2 次，间隔 0.4 × 第几次。
- **配额耗尽特征**（实测旧项目经验）：从发出到"对端断开 / 读错误"超过 20 秒，且没有任何响应 → 不重试，返回 504 并说明"通常表示配额耗尽"。ConnectError 不算（连接都没建立）。
- ReadTimeout（等响应头超时）→ 504，不重试。WriteTimeout → 504。
- 重试用尽 → 502 "无法连接 {上游名}（已重试 N 次）"。
- 读正文时出错：ReadTimeout → 504；其他 → 502。
- **上游繁忙重试**（`retry.busy`，默认开，最多 `busy_attempts` = 3 次，上限 10）：响应头的状态码是 429（限流 / 资源耗尽）、503、529（Claude 过载）时，读掉错误正文、等一会儿再发同一个请求；500 只重试一次；400 / 401 / 403 / 404 等不重试。等待 = `busy_wait` 算式对第 n 次重试（从 1 开始）算出的秒数 × 0.75~1.25 的随机系数，单次不超过 300 秒；上游给了 `Retry-After`（秒）时至少等这么久，它超过 30 秒且超过算出的秒数时不重试。算式（`wait_formula.rs`）：数字、`n`、`+ - * / ^`（`^` 也可写 `**`，右结合，比负号优先：`-2^2` = -4）、括号、`min(…)`、`max(…)`，全角括号 / 逗号 / `× ÷ ＋ －` 当作半角；一个数字就是每次等这么久；结果为负按 0 算、超过 300 按 300 算；第 1~10 次中任何一次算不出有限数（如 `1/(n-1)`）即无效。载入配置与修改配置时校验，无效则拒绝（400，说明哪里写错）；设置页通过 `preview_busy_wait` 命令用同一个解析器实时预览每次的秒数。不管上游过了多久才返回繁忙都重试（实测 Standard 请求也可能先挂 81 秒才回 429）；还没给客户端发出任何字节，所以流式请求同样适用；最多等多久由客户端决定：客户端断开时请求被丢弃，重试随之停止。重试用完仍繁忙时，把最后一次的上游响应原样交给客户端。每次重试记一条 WARN 日志（`kind = upstream_retry`，带 `model`、`status`），重试后成功记一条 INFO（`kind = upstream_retry_ok`，带 `model`、`retries`）。`model` 取自上游地址里的 `/models/{m}:`；OpenAI 兼容接口等地址里没有模型时读请求体的 `model`（去掉 `google/` 这类发布方前缀），都没有时为空。
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
2. 去掉 `google/`、`anthropic/` 前缀
3. Gemini 表命中 → 表值；否则 `gemini-*` → 原样
4. Claude 表命中 → 表值；否则 `claude-…-YYYYMMDD` → `claude-…@YYYYMMDD`；否则 `claude-*` → 原样
5. MaaS 表命中 → 表值；否则含 `/` → MaaS 原样
6. 否则未知（404）

0.1.4 起不再支持本地 Ollama（这个软件只做 Vertex AI 的代理）：旧配置里的 `[ollama]` 段读取时忽略、保存时消失；`ollama/x` 这种带 `/` 的名字按 MaaS 处理。

默认表（2026-11-03 核实，Gemini 已用 countTokens 在 global 实测）：
- Claude：fable-5-1、fable-5、opus-5-5、opus-5、opus-4-8、opus-4-7、opus-4-6、sonnet-5-5、sonnet-5、sonnet-4-6、haiku-5-5 原样；opus-4-5→`@20251101`；sonnet-4-5→`@20250929`；haiku-4-5→`@20251001`
- Gemini：4-carbon、4-hydrogen（这两个 2026-11-14 按官方文档加入，同日用 countTokens 在 global 实测可用；carbon 官方注明面向经过验证的企业客户）、4-argon、3.8-flash、3.7-flash（2027-01-28 下线）、3.6-flash（**2026-11-19 下线**）、3.5-flash、3.5-flash-lite、3.1-flash-lite、3.1-pro-preview、3-flash-preview 原样；3.1-pro、3-pro → 3.1-pro-preview；3-flash → 3-flash-preview
- 已下线、不得出现在默认表：gemini-2.5-pro/flash/flash-lite、2.0-flash(-001)、3-pro-preview
- MaaS：空（官方未给出清单，核实后再加）

## 8. 入口

### 8.1 Anthropic Messages（最高优先级）

- `POST /v1/messages`、`/anthropic/v1/messages`；`POST .../v1/messages/count_tokens`
- 请求体必须是 JSON 对象（否则 400）；`model` 必填非空（否则 400）；解析结果是 Claude → 下面的原样转发；是 Gemini → 按 §14 翻译成 Gemini 原生请求（服务档位照 §9 生效，`X-VertexSwitch-Tier` 同样可用）；其他（MaaS）→ 404。
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
- 按 `model` 分流：Gemini / MaaS 原样转发（见 §3），流式转发时 `text/event-stream`，中途出错按 OpenAI 格式发错误；Claude 走 §10 转换。
- Gemini 兼容接口**绝不能**带档位头（Vertex 返回 400 "Flex API is not supported with chatCompletions API"）。
- Gemini：按 §9 算出档位方案；要发任何档位头（或 `openai_native = always`）时转成原生 generateContent（§13），否则走兼容接口。请求头 `X-VertexSwitch-Tier` 同样可按请求覆盖；走原生时响应带 `X-VertexSwitch-Tier`。
- **实测**（2026-11-03，Rust，global）：Gemini 兼容接口用 `v1beta1` 可用；非流式、流式、工具调用（含流式）、`/openai/v1`、`/chat/completions` 别名均正常。配置为 Flex 时该接口不带档位头，上游按 `ON_DEMAND` 处理；同一配置下 Gemini 原生接口仍为 `ON_DEMAND_FLEX`。兼容接口的响应里用量档位在 `usage.extra_properties.google.traffic_type`。

### 8.4 模型列表

- `GET /v1/models`、`/anthropic/v1/models`：带 `anthropic-version` 头或 `/anthropic/` 前缀 → Anthropic 格式（只列 Claude）：`{"data":[{"type":"model","id","display_name","created_at":"1970-01-01T00:00:00Z"}],"has_more":false,"first_id","last_id"}`；否则 OpenAI 格式（Claude + Gemini + MaaS 别名）：`{"object":"list","data":[{"id","object":"model","created":0,"owned_by":"vertexswitch"}]}`
- `GET /v1/models/{id}`：同上判定格式；未知 → 404
- `GET /v1beta/models`、`/gemini/v1beta/models`：`{"models":[{"name":"models/<id>","displayName","supportedGenerationMethods":["generateContent","streamGenerateContent","countTokens"]}]}`；`GET /v1beta/models/{id}` 单个，未知 → 404

### 8.5 健康检查

`GET /health`（不鉴权）：`{"status":"ok"|"degraded","version","region","gemini_region","gemini_tier","google_token":"ok"|<错误>,"project_id"}`；拿不到 token → 503。

## 9. 服务档位（Gemini 生成请求）

依据：Flex PayGo、Priority PayGo 官方文档。**全部实测通过**（2026-11-03，global；Python 原型与 Rust R1 各测一次）。
Rust 实测时 Priority 请求一次被上游降级为 `ON_DEMAND`（文档所述的无空余容量情形），告警按设计触发。

- 用哪个档位：请求头 `X-VertexSwitch-Tier`（写错时忽略）> `gemini.model_tiers[上游模型 ID]` > `gemini.tier`。配置里的档位名在加载 / 修改时校验，错了拒绝（400 / 保留旧配置）。
- 只有原生接口能带档位头：Gemini 原生入口直接带；OpenAI 入口的 Gemini 请求需要时转成原生格式（§13）。
- 头：`X-Vertex-AI-LLM-Shared-Request-Type: flex|priority`；`shared_only` 时再加 `X-Vertex-AI-LLM-Request-Type: shared`；Flex 加 `X-Server-Timeout: <秒>`（1..1800）。
- Flex：只支持 `global`；不支持 gemini-4-carbon、gemini-4-argon（Flex PayGo 文档的支持列表里没有，2026-11-14 核对）以及 flex_unsupported_models 里另加的模型。gemini-4-hydrogen 在支持列表里。
- Priority：支持 `global`、`us`、`eu`（旧项目误以为只支持 global）；argon 可用，carbon、hydrogen 官方注明支持。
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
- 管理接口的日志条目：`seq, ts_ms, level, target, message, fields`；`fields` 是事件附带的字段（字符串表）。档位告警带 `model`（上游 ID）与 `kind`：`tier_fallback`（不支持该档位，请求前改用 Standard；含 OpenAI 入口 `openai_native = never` 时带不了档位头）或 `tier_mismatch`（trafficType 不符）；上游繁忙重试的日志见 §4（`upstream_retry` / `upstream_retry_ok`）
- 日志、代理自己产生的错误信息按界面语言写中文或英文（配置目录的 `language` 文件，`zh` / `en`，由外壳在界面切换语言时写入，启动时读取；没有该文件时用中文）。上游的错误原样透传
- 桌面程序同时把日志写进配置目录的 `logs\vertexswitch.log`（超过 5 MB 轮换，留一份 `.log.1`），panic 也记录
- 不记录请求/响应正文（含真实对话）

## 12. 管理接口（R4）

- 前缀 `/admin/api/`；**只接受本机连接**（非回环地址 403），并要求 `Authorization: Bearer <管理 token>`（401）。管理 token 与代理的 `server.api_key` 相互独立。
- 管理 token 首次启动时随机生成（48 位十六进制），存系统凭据库（Windows 凭据管理器：`admin-token.VertexSwitch`）；凭据库不可用时退回 `%APPDATA%\VertexSwitch\admin_token` 文件。旧版本留下的该文件会迁入凭据库后删除。获取方式：界面 设置 → 系统 → 复制；或命令行 `vertexswitch-core admin-token`。
- 管理接口、`/health`、`/metrics` 自身不计入请求记录。

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `status` | `/health` 的内容 + `gemini_model_tiers`、监听地址、运行时长、进行中请求数、各后端模型数、配置文件路径 |
| GET | `config` | 当前完整配置 |
| PATCH | `config` | JSON Merge Patch（RFC 7396），`null` 恢复默认；校验 → 写盘 → 立即生效 |
| PUT | `config` | 整体替换，同上 |
| POST | `config/reload` | 从配置文件重新读取并应用 |
| GET | `models` | 三张表（保持声明顺序）+ 来源（用户文件 / builtin） |
| POST | `models/reload` | 重新读取模型表；有误返回 400 并保留原表 |
| GET | `logs?since=&limit=` | 最近日志（环形缓冲 1000 条，INFO 及以上）；`since` 为序号，用于增量拉取 |
| GET | `logs/stream` | 日志 SSE 实时流（`data:` 为一条 JSON 日志） |
| GET | `requests?since=&limit=` | 最近请求记录（环形缓冲 500 条） |
| GET | `requests/stream` | 请求记录 SSE 实时流 |
| GET | `metrics` | JSON 汇总：按 (协议, 后端, 状态码) 的请求数、耗时、token 用量 |
| POST | `test` | `{"model", "prompt"?, "max_tokens"?=32}` 经 OpenAI 入口在进程内发一个最小请求，返回 `ok / status / latency_ms / reply / error`；**会真实调用上游** |

配置修改：
- 认证信息（`credentials_file`、`project_id`）不变时沿用原 token 来源与缓存的 token，否则新建并重启后台刷新；超时 / 重试不变时沿用连接池。
- `server.host` / `server.port` 变化时返回 `restart_required: true`，由外壳重启监听。不论改动来自界面、管理接口还是手改配置文件（热加载），核心都会发一次“监听地址变了”通知（`AppState::listen_changed`），外壳收到后重启监听；只改别的设置不通知。
- 每个请求开始时取一份运行时快照（配置、token 来源、传输、模型表），改配置不影响进行中的请求。

热加载：每 2 秒检查配置文件与用户模型表的修改时间；内容变化则重新加载，内容有误时保留当前配置并告警。

请求记录（每条）：`seq, ts_ms, method, path, protocol, model(上游 ID), backend, tier, stream, status, duration_ms, first_byte_ms, input_tokens, output_tokens, outcome, error`。
- `first_byte_ms`（首字耗时）：从收到请求到发出第一段非空响应体的毫秒数。流式请求里包括排队、繁忙重试和模型思考，之后才是出字时间；非流式约等于 `duration_ms`；没发出任何内容（出错、客户端提前断开）时为 null。
- 在**响应体结束时**落定：`ok`；`error`（状态码 ≥ 400）；`stream_error`（流中由代理补发了错误事件；所有入口与翻译路径的流内错误都由 `ProxyError::stream_event` 生成、以空行开头，记录据此识别）；`cancelled`（响应体未读完就被丢弃，如客户端断开）。
- 用量从响应字节流里取各协议字段的最后一次出现值：Anthropic `input_tokens`(+缓存读写) / `output_tokens`；Gemini `promptTokenCount` / `candidatesTokenCount`(+`thoughtsTokenCount`)；OpenAI `prompt_tokens` / `completion_tokens`。OpenAI→Claude 的流式请求只有客户端开了 `stream_options.include_usage` 才有用量。

`/metrics`（Prometheus 文本，**需要代理 API key**）：`vertexswitch_requests_total{protocol,backend,status}`、`vertexswitch_request_duration_seconds_{sum,count}{protocol,backend}`、`vertexswitch_tokens_total{protocol,backend,kind}`、`vertexswitch_requests_in_flight`、`vertexswitch_uptime_seconds`。

## 13. OpenAI → Gemini 原生转换

用途：OpenAI 入口的 Gemini 请求需要服务档位时（兼容接口不接受档位头）。字段对应照 Vertex 自己的 OpenAI 兼容接口文档，换路后行为尽量不变。
依据：Vertex「OpenAI compatibility」与「Thought signatures」文档、v1 discovery 文档（GenerateContentRequest / Part / FunctionDeclaration / Candidate）。

**实测**（2026-11-04，Rust，global，gemini-3.5-flash-lite，8/8 通过）：
- 请求头 Flex 的非流式、流式（含 `include_usage`）都转原生成功，响应头 `X-VertexSwitch-Tier: flex`，没有 trafficType 不符告警；同模型原生入口 + Flex 的 trafficType 为 `ON_DEMAND_FLEX`。Flex 每次约 7 秒，Standard 约 1 秒。
- 工具调用：第一轮返回 `tool_calls` 且带思考签名（124 字符，在 `extra_content.google.thought_signature`）；带回签名的第二轮正常作答；客户端丢掉签名时填占位值 `skip_thought_signature_validator`，同样正常（没有 400）。
- 管理接口给该模型设 `model_tiers = flex` 后不带请求头即走 Flex，删掉后恢复；Standard 请求仍走兼容接口。

请求（发往 `…/publishers/google/models/{model}:generateContent`，流式为 `:streamGenerateContent?alt=sse`，v1）：
- `system` / `developer` → `systemInstruction.parts[].text`；`user` → role `user`；`assistant` → role `model`；空文本不发。
- 内容：`text`；`image_url` 的 data URL → `inlineData`，其他地址（https / gs）→ `fileData`（按扩展名猜 MIME，猜不出按 image/jpeg）；`input_audio` → `inlineData`（`audio/{format}`）；`file.file_data` → `inlineData`；`file_id` 等 → 400。
- 助手 `tool_calls`（含已弃用的 `function_call`）→ `functionCall{name,args}`；`extra_content.google.thought_signature` → 该部分的 `thoughtSignature`。
- 思考签名：Gemini 3 起（模型名 `gemini-N…`，N ≥ 3）函数调用缺签名会 400。一条助手消息里的调用都没带签名时，给第一个调用填文档给出的占位值 `skip_thought_signature_validator`（只有并行调用的第一个带签名）；更早的模型不填。
- `tool` / `function` 消息 → `functionResponse{name, response:{output}}`，name 用消息自带的或按 `tool_call_id` 从前面的调用里找，找不到 → 400；能解析成 JSON 就按 JSON 放进 output，否则按字符串；连续的工具结果合进同一条 user 内容。
- 工具：`tools[].function` / 已弃用的 `functions` → `functionDeclarations[]{name, description, parametersJsonSchema}`（参数原样当 JSON Schema）；`web_search_options` → `googleSearch`；其他工具类型 → 400。
- `tool_choice`：auto / none / required / validated → AUTO / NONE / ANY / VALIDATED；指定函数 → ANY + `allowedFunctionNames`。
- `generationConfig`：temperature、top_p→topP、n→candidateCount、presence_penalty、frequency_penalty、seed；`max_completion_tokens` 优先于 `max_tokens` → maxOutputTokens；stop → stopSequences；`response_format` json_object → `responseMimeType: application/json`，json_schema → 再加 `responseJsonSchema`。
- `reasoning_effort` → 思考设置（§15）：none / minimal → MINIMAL，low / medium / high → 同名，xhigh → HIGH，其他值忽略；`extra_body.google`（或顶层 `google`）的 `thinking_config` → thinkingConfig（顶层键改 camelCase），与 reasoning_effort 同时给 → 400；`safety_settings` → safetySettings；`cached_content` → cachedContent。
- 其他字段（user、logprobs、stream_options 以外的 OpenAI 专有字段）忽略。

响应：
- `id` = `chatcmpl-{responseId}`；每个候选一个 choice（`index` 取候选的 index）。
- 文本拼成 `content`；`thought: true` 的文本放 `reasoning_content`；`functionCall` → `tool_calls[]`（id 用上游给的，没有就生成 `call_…`；签名放 `extra_content.google.thought_signature`）。
- finish_reason：有工具调用 → `tool_calls`；MAX_TOKENS → `length`；SAFETY / RECITATION / BLOCKLIST / PROHIBITED_CONTENT / SPII / MODEL_ARMOR / IMAGE_* 拦截类 → `content_filter`；其余 → `stop`。提示词被拦（无候选、有 `promptFeedback.blockReason`）→ 一个空 choice，`content_filter`。
- usage：prompt = promptTokenCount + toolUsePromptTokenCount；completion = candidatesTokenCount + thoughtsTokenCount（后者另记 `completion_tokens_details.reasoning_tokens`）；cachedContentTokenCount → `prompt_tokens_details.cached_tokens`。
- 流式：每个上游事件转成 `chat.completion.chunk`；候选第一段先发 role；函数调用整段到达，转成一次带完整 arguments 的 tool_calls 增量；收到 finishReason 发结束块；正常结束后客户端开了 `stream_options.include_usage` 时补一个只有 usage 的块，再发 `[DONE]`。没收到 finishReason 就断了 → 发 OpenAI 格式错误事件（"流式响应提前结束"），不发 `[DONE]`。上游 SSE 里出现 `error.message` → 转成错误事件并结束。
- 上游非 2xx → 按 §5 转成 OpenAI 错误格式。trafficType 核对同 §9。

## 14. Claude 格式 → Gemini 原生转换

让只会说 Claude 格式的客户端（如 Claude Code）调用 Gemini 模型：`/v1/messages` 与 `/v1/messages/count_tokens` 收到 Gemini 模型时，
把 Anthropic Messages 请求翻译成 Gemini 原生 generateContent / countTokens，回复翻译回 Claude 格式（`translate::anthropic_to_gemini`）。

请求：
- `system`（字符串或 text 块）→ `systemInstruction`；`messages` 的 user / assistant → `contents` 的 user / model；空消息跳过。
- 内容块：text → text；image / document 的 `source`：base64 → `inlineData`，url → `fileData`（按扩展名猜 MIME），text → text；
  `cache_control` 一律忽略；历史里的 `redacted_thinking`、服务端工具调用与结果跳过；其他未知类型 → 400。
- tool_use → `functionCall`（`input` → `args`）；tool_result → `functionResponse`，函数名按 `tool_use_id` 在整段历史的 tool_use 里找，找不到 → 400；
  内容的文字合成一段，能解析成 JSON 就按 JSON，`is_error` 时放在 `response.error`，否则放在 `response.output`；内容里的图片 / 文档作为单独的 part 跟在后面。
- 工具：没有 `type` 或 `type: custom` 的工具 → `functionDeclarations`，`input_schema` 去掉 `$schema` 后放进 `parametersJsonSchema`；
  带版本号 `type` 的服务端工具（web_search、bash、text_editor 等）跳过并记 WARN。`tool_choice` auto / any / none → AUTO / ANY / NONE，
  `tool` → ANY + `allowedFunctionNames`；没有可用工具时不发 `toolConfig`。
- `max_tokens` → `maxOutputTokens`；`temperature`、`top_p` → `topP`、`top_k` → `topK`、`stop_sequences` → `stopSequences`。
- `thinking.type = enabled` → 思考设置（§15）：`budget_tokens` < 4096 → LOW，< 16384 → MEDIUM，其余 → HIGH（老模型直接用这个预算），且 `includeThoughts: true`；
  `adaptive` → 只要 `includeThoughts`；`disabled` 或没写 → 不发（用模型默认）。
- countTokens 只发 `contents` / `systemInstruction` / `tools`（Vertex 的 CountTokensRequest 只认这些），回复 `{"input_tokens": totalTokens}`。

思考签名（Gemini 3 起函数调用必须带回，见 §13）：回复里带签名的 `functionCall` 前面插一个 thinking 块，`signature` 为 `vsg1:` + 签名；
客户端把这个块原样带回时，签名挂回它后面的第一个 `functionCall`。不是 `vsg1:` 开头的签名（Claude 自己的）忽略。
需要签名的模型（§13 的判断）在一组函数调用都没带签名时，给第一个填 `skip_thought_signature_validator`。

回复（只取第一个候选）：
- 思考片段（`thought: true`）→ thinking 块（`signature` 为空）；文本 → text 块；相邻同类片段合并；`functionCall` → tool_use 块，ID 自己生成（`toolu_vs…`）。
- `stop_reason`：有工具调用 → `tool_use`；`MAX_TOKENS` → `max_tokens`；安全类（SAFETY、RECITATION、BLOCKLIST、PROHIBITED_CONTENT、SPII、MODEL_ARMOR、IMAGE_*）→ `refusal`；
  提示词被拦截（没有候选、有 `promptFeedback.blockReason`）→ `refusal`；其他 → `end_turn`。`stop_sequence` 恒为 null。
- `usage`：`input_tokens` = promptTokenCount + toolUsePromptTokenCount − cachedContentTokenCount；命中缓存的部分放 `cache_read_input_tokens`；
  `output_tokens` = candidatesTokenCount + thoughtsTokenCount。`id` = `msg_` + responseId，`model` 为客户端写的名字。

流式（`streamGenerateContent?alt=sse`）：首段先发 `message_start`（usage 里 output_tokens 为 0）；文本与思考片段按增量发
`content_block_start` / `content_block_delta`（text_delta / thinking_delta）/ `content_block_stop`，类型变了就换一个块；
`functionCall` 整段到达，先发签名块（一个 `signature_delta`），再发一个 tool_use 块（`input` 为 `{}`，参数用一个 `input_json_delta` 给完）；
带 `finishReason`（或提示词被拦截）的一段之后发 `message_delta`（`stop_reason` 与最终 usage）和 `message_stop`。
上游流里的错误事件、中途断开、没有 `finishReason` 就结束，都按 §5 补发 `event: error`（请求记录为 stream_error）。

**实测**（2026-11-06，`gemini-3.5-flash-lite`，global，Standard）：普通对话（非流式 / 流式）、工具调用来回（思考签名原样带回、签名丢失时补占位值两种都成功）、
流式工具调用（签名经 `signature_delta` 往返）、思考摘要（非流式返回 thinking 块）、计数均正常；带 `$schema`、`additionalProperties` 的 Claude Code 风格
`input_schema` 被 `parametersJsonSchema` 接受。`flash-lite` 流式时上游本身不返回思考片段（只在最后一段带签名），不是翻译问题。

## 15. Gemini 思考设置

客户端没指定思考时代理什么都不发，用模型自己的默认值（Vertex 文档 Thinking 页，2026-11：Gemini 4 Argon、3.5 / 3.6 / 3.7 / 3.8 Flash 为 MEDIUM，
3.5 Flash-Lite、3.1 Flash-Lite 为 MINIMAL，3.1 Pro、3 Flash 为 HIGH；2.5 系列为动态预算）。客户端指定了思考力度时（§13 的 `reasoning_effort`、
§14 的 `thinking`）按模型换成 `generationConfig.thinkingConfig`（`translate::gemini_thinking`）：

- Gemini 3 起（`gemini-<主版本>` ≥ 3）发 `thinkingLevel`（MINIMAL / LOW / MEDIUM / HIGH）。文档：`thinkingBudget` 对 Gemini 3 起已弃用，
  Gemini 4 收到直接 400，Gemini 3 偶尔 400，与 `thinkingLevel` 同时给也报错。
- 各模型支持的等级：Gemini 4、3.7 / 3.8 Flash、3.1 Pro 没有 MINIMAL；3.1 Flash Image 与 3.1 Flash-Lite Image 只有 MINIMAL / HIGH；
  3 Pro Image 只有 HIGH；其余四档都有。请求的等级不支持时换成最近的一档，距离相同取更高的一档（如 Gemini 4 的 MINIMAL → LOW）。
- 更早的模型发 `thinkingBudget`：客户端给了具体预算（Claude 格式的 `budget_tokens`）就用它，否则按力度取 512 / 1024 / 8192 / 24576；
  夹在模型允许的范围内（Pro 128–32768，Flash-Lite 512–24576，Flash 1–24576）。
- `extra_body.google.thinking_config`（OpenAI 格式）是客户端自己写的原生设置，原样照转，不换算。

**实测**（2026-11-06，`gemini-3.5-flash-lite`）：`thinkingLevel` 用大写枚举名（`"LOW"` 等）被 Vertex 接受；Claude 格式 `budget_tokens` 2048（→ LOW）
与 20000（→ HIGH）都成功，HIGH 的输出 token 明显更多；OpenAI 格式 `reasoning_effort: minimal`（→ MINIMAL，经原生接口）成功。
