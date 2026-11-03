# VertexSwitch

Windows 桌面上的 Vertex AI 多协议代理（Anthropic / Gemini / OpenAI → Vertex AI），带 GUI。从零实现。

开工前先读 `docs/HANDOFF.md`：背景、需求清单、目标架构、里程碑都在里面。

## 必须遵守

- 旧项目 vertex-proxy 正在 `127.0.0.1:8787` 运行，位于 `C:\Users\Admin\AppData\Local\Python\pythoncore-3.14-64\Lib\site-packages\vertex_proxy`。不要停止它，不要修改那里的文件；新程序开发期间用 8788 端口。
- 不要把旧代码复制进本仓库。实现依据 Google / Anthropic 官方文档。
- 服务账号密钥、`.env`、含真实对话的日志不得提交。
- 依赖装在项目虚拟环境里，不装到全局 Python。
- 测试默认用 httpx.MockTransport，不调用真实 Vertex；需要真实调用时先问用户。
- 用户主要通过 `/v1/messages` 使用 Claude，这条路径优先。
- 与用户交流用中文。
