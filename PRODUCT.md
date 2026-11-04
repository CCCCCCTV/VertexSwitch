# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

（界面是 Svelte 网页，运行在 Tauri 2 桌面外壳的系统 WebView 里；以 Windows 10/11 为主，macOS / Linux 顺带支持。设计语言不随操作系统变化。）

## Users

- 主要用户：自己在本机跑 AI 编程工具和客户端（Claude Code、OpenAI 兼容客户端、Gemini 客户端）、手里有 Google Cloud 服务账号的开发者。最初是作者本人。
- 可能面向公开用户（GitHub 上下载安装包的人），但**不需要做过多引导**：默认用户懂 Vertex AI、服务账号和 API 代理是什么。

## Product Purpose

在本机提供 Anthropic Messages、Gemini generateContent、OpenAI Chat Completions 三种接口，用 Google Cloud 服务账号把请求转发到 Vertex AI。图形界面负责配置、切换服务档位、查看请求与日志、排查问题；转发本身在后台常驻，不需要界面开着。

成功的标志：客户端只改一个 base URL 就能用；出问题时能在界面里很快看出是哪一环（认证、配额、档位、上游）出了问题。

## Positioning

- Windows 托盘常驻的单进程桌面程序（单个 exe），不是需要自己部署的服务器端聚合网关。
- 原生理解 Vertex AI 的细节：Gemini 服务档位（Standard / Flex / Priority）与按 `trafficType` 核对实际档位、配额耗尽的特征识别、各区域端点规则。
- 依据 Google / Anthropic 官方文档从零实现。

## Operating Context

- 程序常驻托盘；界面是偶尔打开的。用户打开主界面主要是为了：**切换档位 / 修改配置**，以及**排查问题**（看失败请求、日志）。
- 典型场景：在终端里用 Claude Code 等工具时，某个请求失败或变慢，打开界面看原因；或者想省钱时切到 Flex、着急时切到 Priority。
- 托盘菜单本身也能切档位；界面是更完整的控制台。

## Capabilities and Constraints

- 已有页面：概览、请求、日志、模型、设置（含开机自启、管理 token）。
- 数据都在本机：请求记录与日志只保留在内存（本次运行），不记录请求 / 响应内容。
- 界面通过 Tauri 命令调用同进程的核心；在普通浏览器里打开时用示例数据，仅供预览。
- 术语：服务档位（Standard / Flex / Priority）、区域（global / us / eu / 具体区域）、上游模型 ID、别名、trafficType、管理 token、API key。
- **语言：现在只有中文，以后要中英双语**——文案要可抽取，布局要给英文（通常更长）留余地。
- 未决定：是否提供英文版的默认语言选择逻辑；是否做代码签名。

## Brand Commitments

- 名称：VertexSwitch。
- 现有图标：圆角方形、蓝到青绿渐变底、白色"V"与开关圆点（`ui/public/logo.svg`）。是否保留属于视觉决策，未定。
- 没有其他已确认的品牌资产或语气要求。

## Evidence on Hand

- 没有用户评价、客户案例、基准测试或宣传素材，后续界面与文档不得编造。
- 真实可展示的数据只有运行时产生的请求记录、用量与日志。

## Product Principles

1. **后台优先**：转发必须在界面关闭时照常工作；界面是控制台，不是产品本体。
2. **出错时一眼看出哪一环坏了**：认证、配额、档位降级、上游错误要能直接从界面分辨，不需要翻日志猜。
3. **档位切换是一等操作**：切换要快、当前状态要明确，并说明不同区域下的限制。
4. **懂行的用户，少说教**：不做冗长引导，但关键限制（如 Flex 只支持 global、测试请求会产生费用）要在操作旁说清楚。
5. **只展示真实数据**：不做装饰性的假图表、假指标。

## Accessibility & Inclusion

- 跟随系统的浅色 / 深色主题。
- 中英双语预留（见上）。没有确认更具体的无障碍标准。
