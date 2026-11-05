---
name: VertexSwitch
description: 跟着 Windows 强调色（或桌面画面）变色的 Vertex AI 代理控制台
colors:
  accent: "oklch(0.52 0.14 262)"
  accent-hover: "oklch(0.47 0.14 262)"
  on-accent: "oklch(0.99 0.01 262)"
  accent-soft: "oklch(0.91 0.059 262)"
  on-accent-soft: "oklch(0.3 0.119 262)"
  ring: "oklch(0.6 0.14 262 / 0.45)"
  hover: "oklch(0.52 0.14 262 / 0.1)"
  press: "oklch(0.52 0.14 262 / 0.17)"
  bg: "oklch(0.966 0.036 262)"
  surface: "oklch(0.993 0.011 262)"
  panel: "oklch(0.993 0.011 262)"
  surface-2: "oklch(0.952 0.039 262)"
  surface-3: "oklch(0.92 0.05 262)"
  border: "oklch(0.89 0.031 262)"
  border-strong: "oklch(0.8 0.025 262)"
  text: "oklch(0.24 0.017 262)"
  text-2: "oklch(0.44 0.02 262)"
  text-3: "oklch(0.55 0.017 262)"
  series-ok: "oklch(0.7 0.095 155)"
  ok: "oklch(0.52 0.13 155)"
  ok-soft: "oklch(0.94 0.045 155)"
  warn: "oklch(0.55 0.13 70)"
  warn-soft: "oklch(0.95 0.055 85)"
  bad: "oklch(0.53 0.19 27)"
  bad-soft: "oklch(0.94 0.04 25)"
  on-bad: "oklch(0.99 0.005 25)"
  brand-blue: "#3186FF"
  brand-red: "#FF4641"
  brand-yellow: "#FFD314"
  brand-green: "#00B95C"
typography:
  display:
    fontFamily: "Segoe UI Variable Display, Segoe UI, Microsoft YaHei UI, system-ui, sans-serif"
    fontSize: "22px"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.01em"
  metric:
    fontFamily: "Segoe UI Variable Display, Segoe UI, Microsoft YaHei UI, system-ui, sans-serif"
    fontSize: "20px"
    fontWeight: 600
  title:
    fontFamily: "Segoe UI Variable Text, Segoe UI, Microsoft YaHei UI, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
  body:
    fontFamily: "Segoe UI Variable Text, Segoe UI, Microsoft YaHei UI, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
    fontFeature: "tnum"
  body-sm:
    fontFamily: "Segoe UI Variable Text, Segoe UI, Microsoft YaHei UI, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
  label:
    fontFamily: "Segoe UI Variable Text, Segoe UI, Microsoft YaHei UI, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 500
  mono:
    fontFamily: "Cascadia Mono, Consolas, ui-monospace, monospace"
    fontSize: "12.5px"
    fontWeight: 400
rounded:
  bar: "2px"
  check: "5px"
  sm: "8px"
  md: "12px"
  full: "999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  bento-gap: "14px"
  panel-y: "16px"
  panel-x: "18px"
  page-top: "22px"
  page-x: "28px"
components:
  button:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.sm}"
    padding: "0 12px"
    height: "32px"
  button-hover:
    backgroundColor: "color-mix(in oklch, oklch(0.993 0.011 262), oklch(0.52 0.14 262) 8%)"
  button-active:
    backgroundColor: "color-mix(in oklch, oklch(0.993 0.011 262), oklch(0.52 0.14 262) 15%)"
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.sm}"
    padding: "0 12px"
    height: "32px"
  button-primary-hover:
    backgroundColor: "{colors.accent-hover}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.text-2}"
    rounded: "{rounded.sm}"
    height: "32px"
  button-ghost-hover:
    backgroundColor: "{colors.hover}"
    textColor: "{colors.text}"
  button-ghost-active:
    backgroundColor: "{colors.press}"
  button-icon:
    backgroundColor: "transparent"
    textColor: "{colors.text-2}"
    rounded: "{rounded.sm}"
    width: "28px"
    height: "28px"
  segmented:
    backgroundColor: "{colors.surface-2}"
    rounded: "{rounded.full}"
    padding: "3px"
  segmented-item:
    backgroundColor: "transparent"
    textColor: "{colors.text-2}"
    rounded: "{rounded.full}"
    padding: "0 14px"
    height: "28px"
  segmented-item-on:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.sm}"
    padding: "0 10px"
    height: "34px"
  panel:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.md}"
    padding: "16px 18px"
  pill:
    backgroundColor: "{colors.surface-2}"
    textColor: "{colors.text-2}"
    typography: "{typography.label}"
    rounded: "{rounded.full}"
    padding: "0 9px"
    height: "22px"
  pill-ok:
    backgroundColor: "{colors.ok-soft}"
    textColor: "{colors.ok}"
  pill-warn:
    backgroundColor: "{colors.warn-soft}"
    textColor: "{colors.warn}"
  pill-bad:
    backgroundColor: "{colors.bad-soft}"
    textColor: "{colors.bad}"
  pill-accent:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.on-accent-soft}"
  filter-chip:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.full}"
    padding: "4px 12px"
  filter-chip-on:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.accent}"
  nav-rail:
    backgroundColor: "transparent"
    width: "76px"
  nav-indicator-hover:
    backgroundColor: "{colors.hover}"
    rounded: "{rounded.full}"
    width: "52px"
    height: "30px"
  nav-glider:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.on-accent-soft}"
    rounded: "{rounded.full}"
    width: "52px"
    height: "30px"
  nav-badge:
    backgroundColor: "{colors.bad}"
    textColor: "{colors.on-bad}"
    rounded: "{rounded.full}"
    height: "16px"
  tier-highlight:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.on-accent-soft}"
    rounded: "{rounded.sm}"
    padding: "9px 10px"
  banner-bad:
    backgroundColor: "{colors.bad-soft}"
    textColor: "{colors.bad}"
    rounded: "{rounded.sm}"
    padding: "10px 14px"
  tooltip:
    backgroundColor: "{colors.text}"
    textColor: "{colors.bg}"
    rounded: "{rounded.sm}"
    padding: "8px 10px"
    width: "170px"
  brand-mark:
    backgroundColor: "{colors.brand-blue}"
    size: "30px"
---

# Design System: VertexSwitch

## Overview

**Creative North Star: "跟着系统强调色呼吸的 Windows 控制台"**

整个界面由一个种子色生成。种子默认取 Windows 系统强调色（桌面外壳在页面加载前注入 `window.__VS_ACCENT__`，窗口重新获得焦点时重读）；用户也可以在 设置 › 外观 改用六个预设、自定义颜色，或打开“实时取色”。种子只提供 OKLCH 的色相与彩度（`--seed-h`、`--seed-c`，彩度夹在 0.04–0.16），底色、表面、描边、文字、主色、主色容器、悬停层全部按固定的亮度与彩度比例从它推出。frontmatter 里的数值是 CSS 默认种子（色相 262、彩度 0.14、色调强度 1）下的浅色解析值；真正的规范是 `ui/src/app.css` 里的公式。

实时取色有两个范围，用一个分段选择切换（关闭 / 整张壁纸 / 窗口后方）。整张壁纸：让桌面窗口用 `PrintWindow` 完整重绘后取色，被窗口挡住也不影响，Wallpaper Engine 取当前画面。两种范围都按“间隔”设置取样（默认 1 秒，可选无间隔到 30 秒）。窗口后方：桌面（`PrintWindow` 重绘）打底，再让本窗口下面、与它重叠的应用窗口（按前后顺序，最多 12 个，跳过隐藏、最小化、别的虚拟桌面、透明覆盖层与无响应窗口）各自 `PrintWindow` 把自己画进内存，从下往上叠成本窗口背后的样子再取色，一次约 150–200ms；不改本窗口的截屏属性，随时截图录屏都看得到本窗口（早先用 `WDA_EXCLUDEFROMCAPTURE` 在取样瞬间把本窗口排除出截屏，取得勤时截图里窗口总是不见，已弃用）。移动窗口后 200ms 立即重取。每次采样得到一个主色种子和一张 8×5 的颜色场：种子要连续两次取到相近的新颜色（色相差超过 12° 或彩度差超过 0.03 才算新）才切换，避免闪烁；颜色场每格保留色相，亮度统一到主题底色（浅色 0.9、深色 0.25），彩度为原色的 0.85 倍并封顶（浅色 0.11、深色 0.075）再乘色调强度，画成小图后拉伸铺满窗口、四周外扩 60px、模糊 36px，颜色留在它在桌面上的位置，换图时 1.2s 交叉淡入。

气质参照 Windows 11 原生控件、shadcn/ui 的精度与克制、FlClash 的莫奈取色与仪表盘板块：1px 描边的板块平铺在略带色调的底色上，板块没有阴影；信息密度偏高，数字一律用等宽表格数字。导航栏没有自己的底色或分隔线，和内容区共用同一片背景（底色、系统材质或颜色场）。概览是 12 列便当网格，板块按重要性占不同列宽。Windows 11 上窗口默认是 Mica 材质，可切换为 Mica Alt、亚克力或不透明；浅色与深色各一套，跟随系统。

**Key Characteristics:**
- 一个种子色生成全部界面色；状态色固定不随种子变；品牌标是唯一使用固定十六进制颜色的元素
- 表面彩度由用户可调的色调强度（`--tint`，0–2）决定，0 为中性灰
- 12px 圆角板块、8px 圆角控件、1px 描边，板块无阴影
- 76px 左侧图标导航栏，与内容区共用背景，选中药丸在各项之间滑动
- 悬停与按下统一是一层叠加的主色（10% / 17%），可按的东西按下时略微缩小
- 实时取色时窗口背景是一片按桌面位置分布的模糊颜色场；可再叠一张用户选的背景图片（不透明度可调）；两者有其一时板块 80% 不透明
- Segoe UI Variable + 微软雅黑，全局 `tabular-nums`
- 签名组件是请求时间线：按结果堆叠的每分钟柱，失败带斜纹，档位切换画竖线

## Colors

单色相体系：除三种状态色和品牌标外，所有颜色都是种子色相上的不同亮度与彩度。

### Primary
- **种子主色** (accent)：主按钮、复选框选中底、时间线的档位切换竖线与标签、筛选胶囊选中的文字与描边、链接、输入框焦点描边。深色主题提亮到 0.78，彩度降到种子的 0.9 倍。
- **主色悬停** (accent-hover)：主按钮悬停与按下；浅色下更深（0.47），深色下更亮（0.83）。
- **主色上的文字** (on-accent)：主按钮文字、复选框的勾。
- **主色容器** (accent-soft)：导航滑动药丸、档位滑动高亮、筛选胶囊选中底、文本选区、`pill.accent`。
- **主色容器上的文字** (on-accent-soft)：与 accent-soft 配对，不单独使用。
- **焦点环** (ring)：45%（深色 50%）透明的主色，用于所有键盘焦点、输入框焦点光晕、选中的种子色按钮。

### Neutral
- **页面底色** (bg)：窗口背景，导航栏与内容区共用。彩度为种子的 0.26 倍 × 色调强度。半透明材质（`data-mica`）下透明度 0.2，透出系统材质。
- **板块表面** (surface)：按钮、输入框、复选框的不透明底；分段选择中选中段的底。几乎无彩度（0.08 倍）。
- **板块底** (panel)：板块与表头的底。平时等于 surface；实时取色（`data-field`）或有背景图片（`data-bg-image`）时为 80% surface + 20% 透明，让背后的颜色透进来。
- **次级表面** (surface-2)：默认胶囊、默认提示条、分段选择的槽。
- **三级表面** (surface-3)：保留的更深一级表面。
- **悬停层 / 按下层** (hover / press)：主色按 10% / 17% 混入透明，叠在可点的行、导航药丸、ghost 按钮、档位行、日志行、表格行、分段选择未选段上；复选框悬停时作为 4px 外圈。
- **描边** (border) / **强描边** (border-strong)：所有 1px 分隔与外框；强描边用于复选框边框、滚动条悬停，输入框悬停时混入 40% 主色。
- **正文 / 次要 / 弱化文字** (text / text-2 / text-3)：同色相低彩度。次要文字用于说明、表头、未选导航、ghost 按钮；弱化文字用于时间戳、坐标轴、占位符、设置说明小字。

### 状态色（固定）
- **成功绿** (ok, 色相 155)、**降级琥珀** (warn, 色相 70)、**失败红** (bad, 色相 27)，各配一个 `-soft` 容器色，用于胶囊、提示条、日志级别、结果文字、“运行状况”里的图标；`on-bad` 是失败红底上的文字（导航角标）。深色主题有独立的一套亮度值。
- **时间线成功柱** (series-ok)：调和过的鼠尾草绿 `oklch(0.7 0.095 var(--ok-h))`（深色 `oklch(0.64 0.085 …)`）。`--ok-h` 由主题引擎按 Material 3 色彩调和算出：从 155° 往种子色相偏差值的一半、最多 15°，仍一眼是绿色；彩度比“运行中”胶囊低一档。之前用低彩度种子色，暖棕种子下像“未激活”且和中断柱明度接近，已弃用。中断与失败柱用 warn 与 bad。

### 品牌标（固定）
- **Gemini 蓝** (brand-blue)：图标的底色，整颗星芒先铺满这一色。
- **品牌红 / 黄 / 绿** (brand-red / brand-yellow / brand-green)：三团径向晕染，红在上方偏左、黄在左侧、绿在下方，从中心实色渐变到全透明，叠在蓝底上。四色只出现在应用图标里，界面其他地方不得引用。

### Named Rules
**The One Seed Rule.** 界面里任何非状态色都必须写成 `oklch(L calc(var(--seed-c) * k) var(--seed-h))`，或由这类变量 `color-mix` 得到；不要写死十六进制的品牌色或灰色。唯一例外是应用图标的 Gemini 四色。

**The Fixed Status Rule.** 失败红、降级琥珀、成功绿不随种子变化。种子色相与某个状态色相距不到 35° 时，根元素带 `data-seed-near`，链接改用正文色并常驻下划线，主色不再承担“可点击”的含义。

**The Tint Only Touches Ground Rule.** 色调强度只乘在底色、表面、描边的彩度和颜色场的彩度上；文字、主色、悬停层、状态色不受它影响。

**The Overlay State Rule.** 悬停与按下不换底色，而是叠一层主色（hover 10%、press 17%）；普通按钮是 surface 混 8%（按下 15%）主色、描边混 35% 主色。新加的可点元素沿用这两层，不另造灰色悬停。

## Typography

**Display Font:** Segoe UI Variable Display（后备 Segoe UI、Microsoft YaHei UI、system-ui）
**Body Font:** Segoe UI Variable Text（后备同上）
**Label/Mono Font:** Cascadia Mono（后备 Consolas、ui-monospace）

**Character:** 系统原生字体，不引入网页字体；Display 光学尺寸只用于页面标题与时间线的总数，其余一律 Text。

### Hierarchy
- **Display** (600, 22px, 1.25, -0.01em)：每页唯一的页面标题。
- **Metric** (600, 24px)：时间线顶部的请求总数这类单个关键数字；用量、基准数据小格里的数字 18px。
- **Title** (600, 14px)：板块标题；设置页的分区标题放大到 15px。
- **Body** (400, 14px, 1.5)：默认正文与控件文字；按钮与分段选择为 500。
- **Body small** (13px)：提示条、筛选胶囊、表单字段名（500）、结果文字、时间线摘要行。
- **Label** (500–600, 12px)：表头、导航文字（选中 600）、胶囊、说明小字、图例。坐标轴、时间线标签与接入地址的变量名 11px；导航角标 10.5px / 700。
- **Mono** (12.5px)：地址、模型 ID、日志正文、区域等可复制的技术值；状态胶囊里的地址缩到 12px。

### Named Rules
**The Tabular Numbers Rule.** `font-variant-numeric: tabular-nums` 在根上全局开启；数字列一律右对齐（`td.num`）。

**The No Uppercase Rule.** 不使用全大写或加宽字距的小标签；层级靠字号与字重，不靠字距。

## Layout

窗口默认 1120 × 740px，最小宽 820px。外壳是两列网格：76px 导航栏 + 内容区，整体 `isolation: isolate` 并裁切溢出，颜色场作为它的 `::before` 铺在最底层。导航栏竖向排列，上下内边距 16px，项间距 6px，顶部 30px 品牌标下边距 14px。内容区内边距上 22px、左右 28px、下 28px，自身滚动。

每页顶部一行：左边页面标题，右边是操作或状态胶囊（间距 16px，下边距 16–18px）。设置页这一行吸顶：用负外边距铺满内容区宽度（`margin: -22px -28px 16px`，`padding: 22px 28px 12px`），底色为 bg 加一条底描边；在半透明材质下改为 92% 不透明的底色 + `blur(24px) saturate(1.2)`；实时取色时改为 35% 不透明 + `blur(28px) saturate(1.2)`，让颜色场连贯，滚过的内容仍看不清。

概览是 12 列便当网格，间距 14px：时间线 8 列、档位 4 列；下一行三个 4 列板块（运行状况、接入地址、用量），一屏放下不用滚动；“测试连接”不再占一行，改为页头右上角的按钮（状态胶囊在左侧换行，按钮固定不动），点开是居中的模态弹窗：宽 460px、圆角 16px，背后整页蒙一层种子色淡影并轻微模糊；弹窗里是标题与关闭按钮、一句说明、模型选择和“发送”、结果。点蒙层、按关闭或 Esc 都能关掉；液态玻璃开着时弹窗也是玻璃。窗口窄于 1080px 时 4 列板块改为 6 列、时间线占满 12 列。表格页（请求、模型）把整张表放进无内边距的板块；请求页窄于 900px 时隐藏档位列、搜索框缩到 160px。设置页分区最大宽度 920px、分区间距 14px，表单按两列或三列排布，列间距 16px。

控件间距以 4 / 8 / 10 / 12px 为主；板块内边距 16px 18px，板块标题行下边距 12px。

## Elevation & Depth

扁平，靠色调分层。从下到上是：窗口背景（不透明底色、薄薄一层种子色下的系统材质，或模糊颜色场）→ 板块（最亮的一层，1px 描边）→ 叠加的悬停 / 按下层。深度来自描边与表面亮度差，不来自阴影。半透明材质下只有底色变薄，板块仍不透明；实时取色时颜色场盖住系统材质，板块降到 80% 不透明，表头用 10px 模糊挡住滚过去的行。

### Shadow Vocabulary
- **焦点光晕** (`box-shadow: 0 0 0 3px var(--ring)`)：输入框与下拉框获得焦点，同时描边换主色；其余元素用 `outline: 2px solid var(--ring)`，偏移 2px。
- **选中环** (`box-shadow: 0 0 0 2px var(--ring)`)：设置页选中的种子色按钮。
- **浮起小控件** (`--shadow`，浅色 `0 1px 2px oklch(0.2 calc(var(--seed-c) * 0.3) var(--seed-h) / 0.14)`，深色 `0 1px 2px oklch(0 0 0 / 0.45)`)：分段选择中选中的一段、日志页的“跳到最新”、模型页选中的标签。
- **主按钮悬停光晕** (`box-shadow: 0 4px 12px -4px color-mix(in oklch, var(--accent) 70%, transparent)`)：只在主按钮悬停时出现，按下时收回。
- **浮层阴影** (`box-shadow: 0 4px 12px oklch(0 0 0 / 0.18)`)：时间线悬停提示框。

### Named Rules
**The Flat Panel Rule.** 板块、表格、提示条永远没有阴影；阴影只给浮在内容上方的元素（提示框、跳到最新）、分段选中段、主按钮悬停和焦点 / 选中环。 例外：液态玻璃模式下玻璃板块带一圈内高光和柔和投影，表现“浮在画面上”。

**The Opaque Panel Rule.** 系统材质只作用于底色；承载文字的板块不透明。实时取色时板块可降到 80% 不透明（背后是已经模糊过的颜色场），但不能更低。 例外：液态玻璃模式下板块是玻璃，浓度随本软件背景（底色、颜色场、背景图片）的亮度自动调（浅色主题 30%–72%，深色 34%–78%），保证字的对比度。

## Shapes

两档圆角：12px 给板块；8px 给按钮、输入框、提示条、档位行与档位高亮、提示框。复选框 5px；时间线柱子与图例色块 2px。全圆角（999px）给胶囊、筛选胶囊、分段选择的槽与段、导航药丸与滑块、导航角标、种子色按钮。描边统一 1px（复选框 1.5px）。设置页的折叠箭头用两条 1.5px 描边画成，不用字符。

## Components

### Buttons
- **Shape:** 8px 圆角，高 32px，左右 12px，字重 500，图标与文字间距 6px。
- **Default:** surface 底 + 1px 描边；悬停混 8% 主色、描边混 35% 主色；按下混 15% 主色并缩到 0.97。颜色过渡 0.15s `cubic-bezier(0.2, 0, 0, 1)`，缩放 0.12s。
- **Primary:** 主色底与描边，on-accent 文字；悬停换 accent-hover 并带主按钮悬停光晕，按下收回光晕。每个板块最多一个。
- **Ghost:** 无边无底，次要文字色；悬停叠 hover 层并变为正文色，按下换 press 层。图标按钮是 28px 方形 ghost。
- **Link:** 无边无底无内边距，link 色文字，悬停下划线（偏移 3px），不缩放；种子近状态色时常驻下划线并改用正文色。只用于句中的行内操作（打开目录、复制）。
- **More（跳转）:** “全部请求 ›”这类去别处的入口：link 色 13px / 600 文字 + 15px 右箭头，26px 高全圆角，平时无底，悬停叠 hover 层、按下 press 层，不加下划线；放在板块标题行右端时右外边距 -6px 与板块边缘对齐。
- **Disabled:** 透明度 0.5，不响应悬停。

### Segmented Control
- 用于互斥的少量选项（实时取色：关闭 / 整张壁纸 / 窗口后方）。槽是 surface-2 底、1px 描边、全圆角、3px 内边距、段间距 2px。
- 每段高 28px、左右 14px、全圆角，次要文字色；悬停叠 hover 层，选中段为 surface 底、正文色、浮起小控件阴影。语义上是 `radiogroup`。

### Chips
- **状态胶囊:** 高 22px、全圆角、12px / 600，左右 9px；默认 surface-2 底次要文字，`ok / warn / bad / accent` 变体用对应 soft 底与实色字。可带 8px 圆点（currentColor）。
- **筛选胶囊:** 请求页与日志页的筛选按钮组，间距 4px，全圆角、13px、上下 4px 左右 12px，其余同默认按钮；选中时 accent-soft 底、主色描边与主色文字。

### Cards / Containers
- **Corner Style:** 12px。
- **Background:** panel（平时等于 surface；实时取色时 80% 不透明）。
- **Shadow Strategy:** 无，见 Elevation & Depth。
- **Border:** 1px border。
- **Internal Padding:** 16px 18px；放表格或日志时内边距为 0 并裁切溢出。
- **板块标题行:** 16px 线性图标 + 14px 标题 + 右对齐的弱化说明或链接按钮，次要文字色，间距 8px。

### Inputs / Fields
- **Style:** 高 34px，surface 底，1px 描边，8px 圆角，左右 10px；悬停描边为强描边混 40% 主色。下拉框去掉原生外观，用内联 SVG 箭头，右侧留 30px。
- **Focus:** 描边换主色，外加 3px 焦点光晕。
- **Checkbox:** 自绘 18px 方框，5px 圆角，1.5px 强描边；悬停加 4px hover 外圈；选中为主色底，勾用 on-accent，0.15s 缩放出现。单选框与滑块用原生控件加 `accent-color: var(--accent)`。
- **Radio:** 自绘 16px 圆圈，1.5px 强描边；悬停加 4px hover 外圈；选中描边换主色，中间 8px 主色实心圆点（0.18s 放大出现）。不用浏览器默认的灰黑单选框。
- **Disabled:** 透明度 0.55。

### Navigation
- 76px 宽的竖向导航栏，没有单独的底色和分隔线。品牌标、各项与选中药丸放在一条 64px 宽的“导轨”里（内边距 12px 0 10px，两种模式一致，选中药丸按导轨内的位置算）；液态玻璃时导轨就是一枚包住图标的悬浮玻璃胶囊（32px 圆角），不通到底。
- 每项 64px 宽，是 20px 线性图标（描边 1.75）+ 下方 12px 文字。图标放在 52×30px 的药丸里：悬停药丸叠 hover 层、按下叠 press 层并缩到 0.92；整个按钮本身不变底色、不缩放。
- 选中由一块独立的 accent-soft 滑块（52×30px，全圆角）滑到当前项：0.38s `cubic-bezier(0.16, 1, 0.3, 1)`，首次定位不做动画。选中项图标换 on-accent-soft，文字正文色、600。
- 请求项在最近 10 分钟有失败时显示失败红数字角标（16px 高、全圆角、10.5px / 700）。
- 有新版本时设置项右上角显示 9px 主色圆点（外圈 2px surface 色描边），悬停提示“有新版本”。

### Banners
- 8px 圆角、上下 10px 左右 14px、13px 文字，左侧 16px 状态图标，间距 10px；`bad / warn / ok` 用 soft 底与实色字，默认 surface-2。用于认证失败、端口占用、区域限制等需要就地说明的问题。

### Tables
- 表头 12px / 500 次要文字，吸顶，panel 底加 10px 背景模糊；单元格 9px 12px，行间 1px 描边，最后一行无描边；行悬停叠 hover 层（0.12s）。数字列右对齐。概览“用量”里的小表去掉表头底色和行悬停，单元格 6px 4px。

### 档位选择器（概览）
- 单选列表，行间距 6px，每行 8px 圆角、9px 10px 内边距、粗体名称 + 12px 次要说明。三行是同一组选项卡：未选行有一圈 1px border 色细描边，选中行描边变透明、由滑动高亮填满；未选行悬停叠 hover、按下叠 press。选中行背后是一块 accent-soft 高亮，在选项之间以 0.38s `cubic-bezier(0.16, 1, 0.3, 1)` 滑动；选中行文字变为 on-accent-soft（说明 80% 透明度）。档位切换是一等操作，这块滑动高亮是它的反馈。

### 外观设置（设置页）
- 种子色按钮：36px 高、全圆角；预设为 36px 圆形，内含 20px 色块（带 12% 黑色内描边）；“跟随 Windows 强调色”与“自定义”带文字。选中时描边换主色并加选中环。
- 下面依次是实时取色分段选择（打开时右侧出现“间隔”下拉框：无间隔 / 0.5 / 1（默认）/ 2 / 5 / 10 / 30 秒，无间隔即取完马上取下一次）+ 说明、色调强度滑块（中性 ↔ 浓，0–2，步长 0.05，可恢复默认）、窗口材质下拉框（Mica / Mica Alt / 亚克力 / 不透明，仅 Windows 11 显示）、背景图片（48×30 缩略图 + “选择图片 / 更换图片”按钮 + 带垃圾桶图标的“删除图片”按钮，两个都是带描边的普通按钮、不折行；有图时多一条不透明度滑块 5–100%，默认 35%），说明小字跟随当前选项。
- 背景图片复制到配置目录（`background.<扩展名>`，≤30 MB），铺在颜色场之上、内容之下（外壳的 `::after`，`cover` 居中），不透明度用 `--bg-image-opacity`。

### 界面语言（设置 → 系统）
- 中文 / English 两套文字，设置 → 系统第一行是分段选择：跟随系统 / 中文 / English，选项名用各自的语言写，标签写成“语言 / Language”，看不懂当前语言也找得到。立即生效，只存在这台电脑上（`localStorage`）；跟随系统按 WebView 报告的系统语言，以 zh 开头即中文，否则英文。
- 文字都在 `ui/src/lib/locales/`：zh.ts 为准，en.ts 用同一组键（少一条类型检查就报错）；英文单复数写成“单数|复数”。
- 核心与外壳的文字也跟随界面语言：日志、返回给客户端的错误、配置 / 模型表校验错误、认证错误、托盘菜单、`/metrics` 的说明行（上游原样透传的错误除外）。界面每次启动和切换都调 `set_language`，语言记在配置目录的 `language` 文件里，下次启动（含无界面的 `vertexswitch-core`）一开始就是对的语言。核心里用 `tr!("中文", "English", 参数…)` 写文字（`vertexswitch_core::i18n`）；已经写下的日志和请求记录保持当时的语言。
- 界面不解析核心的文字：日志带结构化字段 `fields`，档位降级日志有 `kind`（`tier_fallback` 请求前改用了 Standard，`tier_mismatch` 上游实际档位不符）和 `model`。概览的“运行状况”看这两种，另外列出上游繁忙、重试后成功的请求（`kind = upstream_retry_ok`，标题“上游繁忙，已自动重试 · 模型”，重试用完仍失败的已经作为失败请求列出）；时间轴的“降级”只数 `tier_mismatch`；日志页在消息后用浅色显示除 `kind` 外的字段。
- 英文比中文长：用量小格写成 Tokens in / Tokens out；模型页每行的档位下拉框等宽（`width: 100%`，最小 168px），选项长短不同也不会让某一行变宽。

### 检查更新（设置 → 系统）与日志文件（日志页）
- 外壳的 `check_update` 查 GitHub 的 `releases/latest`（User-Agent 只有 `VertexSwitch/版本号`，15 秒超时；还没有发布过版本时返回 404，视为“还没有发布过版本”）。版本号只比较“主.次.修订”三段数字，带预发布后缀的不算新版。发布页链接只接受本仓库 `releases/` 下的地址，由外壳用系统浏览器打开。
- 界面（`lib/update.svelte.ts`）启动 10 秒后查一次，之后每天一次；“自动检查更新”默认开，关掉记在 `localStorage`。设置 → 系统的“版本”一行：已是最新时是“检查更新”链接 + 浅色结果；有新版时是粗体“有新版本 x.y.z” + “前往下载”链接，同时导航的设置项出现圆点。只提示，不自动下载安装。
- 日志在日志页之外同时写进配置目录的 `logs\vertexswitch.log`（INFO 及以上，与日志页相同，不含请求 / 响应正文；时间为 UTC），超过 5 MB 改名为 `.log.1` 再新开，只留一份旧的。panic 也记一条 ERROR。日志页标题右侧有“打开日志文件夹”链接。
- 托盘的勾选与提示文字每 3 秒和配置对一次，只有变化时才写给系统；点了托盘里的档位时总会重写一遍（系统会自行切换被点项的勾选）。

### 液态玻璃（设置 → 外观的开关）
- 打开后侧边栏、所有板块和页头的状态胶囊变成玻璃，透出并折射本软件自己的背景：底色、实时取色的颜色场、背景图片都照常显示在下面，玻璃带上它们的颜色。不读窗口后方的画面（试过，后方应用的内容和本窗口混在一起显得怪，已放弃）。
- 颜色来源照常（系统强调色 / 预设 / 自定义 / 实时取色）。玻璃浓淡由 `--glass-lum` 决定：底色或颜色场的亮度（OKLCH L³），有背景图片时按不透明度混入图片的平均亮度（载入时缩到 16×16 求得）。
- 玻璃：所有 `.panel` 与导航下的竖条底座（`.glass`，26px 圆角，四周内缩 8 / 6px）。`backdrop-filter` 依次为按元素尺寸生成的折射滤镜（`--glass-filter`：圆角矩形位移贴图，边缘 24px 内往里取样、像凸透镜放大，最大偏移 20px；只有 Chromium/WebView2 支持）、`blur(6px) saturate(1.8) brightness(1.04)`。折射和模糊只在背后有画面（实时取色的颜色场或背景图片）时才做；背后只是均匀底色时它们看不出区别，却让滚动每一帧都重算（240Hz 屏幕上设置页从满帧掉到约 170 帧），这时只留 `saturate(1.8) brightness(1.04)`，玻璃浓淡不变，只少了边缘那圈几乎看不见的弯折。底色 `--glass-fill` 由 `--glass-lum`（帧颜色场的平均亮度）决定浓淡。 页头状态胶囊是小玻璃片：只模糊 8px、提饱和、加亮边，不做折射；成功 / 失败胶囊混 14% 状态色。
- 光感：顶边 1px 亮内高光（浅色 70% 白、深色 22% 白）、左边与四周更弱的内描边、`0 12px 32px -14px` 柔和投影；内容下方一层 140° 斜光（左上亮、右下微亮）。页面大标题加一圈 surface 色柔光。

### 接入地址（概览）
- 每个协议一块内嵌容器（surface-2 70%，10px 圆角，悬停混 8% 主色），名字、地址、复制按钮在同一块里。 地址那一行是一个只读代码框（surface 底，8px 圆角，32px 高，等宽 12.5px），整行可点、点击复制地址。
- 名字旁是环境变量小标签（20px 高全圆角，1px 描边，11px 等宽字；悬停换主色容器）：ANTHROPIC_BASE_URL / OPENAI_BASE_URL / GOOGLE_GEMINI_BASE_URL，点一下复制 `KEY=VALUE`，标签短暂变成“已复制”；提示里写明哪些客户端读这个变量。

### 运行状况 / 用量的空状态（概览）
- 标题固定为中性的“运行状况”（不随状态改名）；标题图标平时是脉搏，有失败或降级时换成告警色三角，下面列出这些问题。
- 健康时：居中一枚 52px 圆形 ok-soft 底的盾牌（ok 色），下面“一切正常”与说明，再一排三格基准数据：进行中请求、最近 30 分钟请求数、平均耗时（都来自真实记录，没有数据显示“—”）。
- 用量：三个总数小格（请求、输入 token、输出 token）始终显示，没转发过就是 0；有数据后下面再列按后端的明细；没有数据时这里是一个撑满剩余高度的虚线占位框（1px border-strong 虚线、8px 圆角），居中写着明细会出现在这里，卡片下半部不空、有数据后表格接上来也不跳。不显示预估费用：没有可靠的价格表，档位不同价格也不同。
- 时间线一个请求都没有时，横向网格降到 35% 不透明。

### 标题栏
- 保留 Windows 原生标题栏（贴靠布局、拖动、读屏照常），不画标题栏上的小图标和标题文字（界面左上角已有品牌标），任务栏与 Alt+Tab 不受影响。
- 标题栏底色跟界面最上面那条背景一致：有颜色场用它最上一行的平均色，否则用底色；有背景图片按不透明度混入图片上沿；Mica 下没有颜色场和背景图片时交还系统（标题栏也是 Mica）。深色主题时标题栏按钮用浅色图标。

### 请求时间线（签名组件）
- 最近 30 分钟、每分钟一根柱，高 168px，柱宽为槽宽减 3px（最小 2px），圆角 2px。每分钟都有一个 3px 高的 border 色底座，没有请求的时段读作“0”而不是没画出来。按结果堆叠：成功 series-ok、中断 warn、失败 bad 加 45° surface 色斜纹（状态不只靠颜色区分）。
- 档位降级在柱顶画一个 warn 色小三角；档位切换时刻画一条主色虚线竖线（1.5px，`4 3`）并标“切到 X”（11px / 600 主色），出现时自上而下以 0.7s 画出。
- 横向网格为 `2 4` 虚线 border 色，坐标轴 11px 弱化文字。悬停时其他柱降到 0.4 透明度，显示反色提示框（text 底、bg 字、8px 圆角、170px 宽、浮层阴影）。
- 顶部一行：24px Metric 总数 + 失败计数 + 右对齐图例（9px 色块，2px 圆角）。

### 品牌标（应用图标）
- 形状：以中心为原点的十二道锥形光芒，四条长的指向正上下左右（长 45.4，根宽 14，端头 1.6 圆角），其余八道较短（长 22–29，根宽 10–11，端头 2.2 圆角），角度不等距，中心一个半径 9 的圆把光芒连起来。整颗星作为蒙版，下面铺 Gemini 2025 四色：#3186FF 蓝底，红、黄、绿三团径向晕染（见 Colors › 品牌标）。没有底板、没有外框、没有投影，透明背景。
- 小尺寸规则：16–24px 一律用小尺寸版——只有八道光芒（四长四短）、更粗（根宽 20 / 16，端头 3–3.4）、中心圆半径 12，避免细芒在小像素下糊掉；32px 及以上用完整版。
- 来源与生成：`src-tauri/icons/source/icon.svg`（完整版）与 `icon-small.svg`（小尺寸版）是唯一来源；`npm run icons`（`scripts/build-icons.mjs`）生成多尺寸 ICO（16/20/24 帧用小尺寸版）、ICNS、各尺寸 PNG、托盘用的 16/20/24/32px PNG，并把完整版复制到 `ui/public/logo.svg`。窗口与托盘按当前缩放取正好合适尺寸的帧，不让系统缩放 256px 大图。
- 界面里：导航栏顶部 30px 显示 `logo.svg`。

## Do's and Don'ts

### Do:
- **Do** 从 `--seed-h` / `--seed-c` 推出所有非状态色，并让底色、表面、描边的彩度乘上 `--tint`。
- **Do** 浅色、深色各给一套亮度；新增颜色时两套都要写。
- **Do** 悬停用 `--hover`、按下用 `--press`，可按的控件按下时缩到 0.97（导航药丸 0.92）。
- **Do** 用 lucide 线性图标（板块标题 16px、导航 20px / 描边 1.75），状态图标用 OctagonX（失败）、TriangleAlert（降级 / 警告）、CircleCheck（正常）。
- **Do** 让失败在颜色之外还有第二个信号（斜纹、图标、文字）。
- **Do** 颜色过渡用 0.12–0.15s `cubic-bezier(0.2, 0, 0, 1)`；有方向的移动（导航滑块、档位高亮、竖线画出）用 `cubic-bezier(0.16, 1, 0.3, 1)`；尊重 `prefers-reduced-motion`（全部过渡与动画关闭）。
- **Do** 少量互斥选项用分段选择，选项多或有长说明时用下拉框。
- **Do** 技术值（地址、模型 ID、区域）用等宽字体，旁边给复制按钮。
- **Do** 改图标只改 `src-tauri/icons/source/` 下的两份 SVG，再跑 `npm run icons`；24px 及以下用小尺寸版。

### Don't:
- **Don't** 给板块、表格或提示条加阴影；层次用表面亮度与 1px 描边表达。
- **Don't** 让系统材质透到板块里：只有底色可以半透明；颜色场下板块最低 80% 不透明。
- **Don't** 给导航栏单独的底色或分隔线；它和内容区共用背景。
- **Don't** 用种子色表示成功、失败或降级；状态色固定，种子近状态色时链接退回正文色。
- **Don't** 在界面里使用图标的 Gemini 四色；品牌四色只属于应用图标。
- **Don't** 把概览做成等大卡片网格；板块按重要性占 4 / 8 / 12 列。
- **Don't** 引入网页字体或全大写加字距的小标签。
- **Don't** 画装饰性的假图表或假指标，图表只画运行时的真实请求。
