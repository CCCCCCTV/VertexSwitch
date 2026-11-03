# VertexSwitch

桌面上的 Vertex AI 多协议代理（Anthropic / Gemini / OpenAI → Vertex AI），带 GUI。以 Windows 为主，技术栈 Rust + Tauri 2 + TypeScript。从零实现。

开工前先读 `docs/HANDOFF.md`（背景、需求、架构、里程碑）和 `docs/SPEC.md`（已核实的行为规格）。

## 必须遵守

- 旧项目 vertex-proxy 正在 `127.0.0.1:8787` 运行，位于 `C:\Users\Admin\AppData\Local\Python\pythoncore-3.14-64\Lib\site-packages\vertex_proxy`。不要停止它，不要修改那里的文件；新程序开发期间用 8788 端口。
- 不要把旧代码复制进本仓库。实现依据 Google / Anthropic 官方文档。
- 服务账号密钥、`.env`、含真实对话的日志不得提交。不要读取或打印密钥文件内容。
- Python 依赖装在项目虚拟环境里（`uv`），不装到全局 Python；Rust 依赖由 Cargo 管理。
- 测试默认用假上游（Python: httpx.MockTransport；Rust: wiremock），不调用真实 Vertex；需要真实调用时先问用户。
- 用户的服务账号目前只能调用 Gemini；真实验收优先用便宜的 `gemini-3.5-flash-lite`，探测模型是否可用用 countTokens。
- 用户主要通过 `/v1/messages` 使用 Claude，这条路径优先（但只能用假上游测试）。
- Python 原型（`src/vertexswitch/`、`tests/`）是参考实现，Rust 版达到同等功能（R3）后删除。
- 与用户交流用中文。
