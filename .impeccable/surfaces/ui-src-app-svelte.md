---
version: 1
slug: "ui-src-app-svelte"
primary_target: "ui/src/App.svelte"
related_targets: ["ui/src/pages/Overview.svelte","ui/src/pages/Requests.svelte","ui/src/pages/Logs.svelte","ui/src/pages/Models.svelte","ui/src/pages/Settings.svelte"]
---

# Surface brief：VertexSwitch 管理界面（概览 / 请求 / 日志 / 模型 / 设置）

Scope：桌面程序主窗口的全部页面。Mode：Operate。

Audience / job：懂 Vertex AI 的开发者；打开界面主要为了切档位、改配置、排查问题（见 PRODUCT.md）。

Direction 来源：用户在方向轮里否掉了掷出的方向，选择“Windows 11 原生”为底，并点名参照 shadcn/ui（精度与克制）和 FlClash 旧版（Material You 莫奈取色、仪表盘板块）。用户钉住的方向优先于掷骰结果。

## Direction contract

THESIS：一个跟着系统强调色变色的 Windows 控制台——整个界面的色调由一个种子色生成（默认取 Windows 强调色），概览是大小不一的功能板块；拒绝“灰底加一个蓝色强调色、整齐等大卡片”的类别默认。

OWN-WORLD：OKLCH 色调体系：底色、表面、描边、导航选中、主按钮全部由种子色的色相生成（表面彩度由用户可调的“色调强度”决定：0 为中性灰，默认 1 时底色约 0.036，最高 2；用户 2026-11-04 明确选择了可调而非固定的低彩度；主色 0.12–0.16），浅色 / 深色各一套；Windows 11 上窗口材质可选 Mica / Mica Alt / 亚克力 / 不透明（默认 Mica），只有底色半透明、面板不透明（用户 2026-11-04 选择）；可打开“实时取色”（整张壁纸 / 窗口后方，用户 2026-11-04 要求两种都要），种子取主色，同时把 8×5 颜色场模糊铺满背景、保留颜色位置，此时面板最低 80% 不透明；状态色（失败红、降级琥珀、成功绿）固定不随种子变。圆角 12px 板块、8px 控件、1px 描边无阴影堆叠；Segoe UI Variable + 微软雅黑，数字一律等宽表格数字。导航栏与内容区共用背景（没有单独底色和分隔线，用户 2026-11-04 觉得原来“格格不入”），选中药丸在各项间滑动；悬停 / 按下统一叠主色层。应用图标：星芒 × Gemini 2025 四色（用户 2026-11-04 选 M3），16–24px 用小尺寸版。

STORY：用户一眼看到服务、认证、当前档位是否正常；在档位板块里直接切换；看时间线确认切换生效、有没有失败和降级；需要细节时去请求 / 日志页。

FIRST VIEWPORT：左侧 76px 图标导航栏（与内容区同一片背景，滑动药丸指示 + 下方文字）；右侧顶部一行：页面标题 + 服务状态胶囊（地址、认证）。概览为 12 列便当网格：第一行“请求时间线”（8 列，签名动作）+“Gemini 档位”（4 列，单选列表 + 区域限制提示）；第二行“运行状况”（4 列，失败与降级）、“接入地址”（4 列，复制按钮）、“用量”（4 列，按后端的请求数与 token）；第三行“测试连接”（12 列）。

FORM：用户钉选方向（canon 加参照：shadcn/ui、FlClash）；签名动作 = 请求时间线：每分钟请求按结果堆叠，失败 / 降级用状态色，档位切换时刻画竖线标记。seed: 628148a936c0d866

FINISH：unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Unresolved

- 中英双语：本轮文案仍为中文，布局给英文留余地；i18n 抽取另行安排。
