---
name: VertexSwitch
description: 跟着 Windows 强调色变色的 Vertex AI 代理控制台
colors:
  accent: "oklch(0.52 0.14 262)"
  accent-hover: "oklch(0.47 0.14 262)"
  on-accent: "oklch(0.99 0.01 262)"
  accent-soft: "oklch(0.91 0.059 262)"
  on-accent-soft: "oklch(0.3 0.119 262)"
  ring: "oklch(0.6 0.14 262 / 0.45)"
  bg: "oklch(0.966 0.036 262)"
  rail: "oklch(0.945 0.048 262)"
  surface: "oklch(0.993 0.011 262)"
  surface-2: "oklch(0.952 0.039 262)"
  surface-3: "oklch(0.92 0.05 262)"
  border: "oklch(0.89 0.031 262)"
  border-strong: "oklch(0.8 0.025 262)"
  text: "oklch(0.24 0.017 262)"
  text-2: "oklch(0.44 0.02 262)"
  text-3: "oklch(0.55 0.017 262)"
  series-ok: "oklch(0.72 0.045 262)"
  ok: "oklch(0.52 0.13 155)"
  ok-soft: "oklch(0.94 0.045 155)"
  warn: "oklch(0.55 0.13 70)"
  warn-soft: "oklch(0.95 0.055 85)"
  bad: "oklch(0.53 0.19 27)"
  bad-soft: "oklch(0.94 0.04 25)"
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
    backgroundColor: "{colors.surface-2}"
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
    backgroundColor: "{colors.surface-2}"
    textColor: "{colors.text}"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.sm}"
    padding: "0 10px"
    height: "34px"
  panel:
    backgroundColor: "{colors.surface}"
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
  filter-chip-on:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.accent}"
    rounded: "{rounded.full}"
    padding: "4px 12px"
  nav-rail:
    backgroundColor: "{colors.rail}"
    width: "76px"
  nav-indicator-active:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.on-accent-soft}"
    rounded: "{rounded.full}"
    width: "52px"
    height: "30px"
  banner-bad:
    backgroundColor: "{colors.bad-soft}"
    textColor: "{colors.bad}"
    rounded: "{rounded.sm}"
    padding: "10px 14px"
---

# Design System: VertexSwitch

## Overview

**Creative North Star: "跟着系统强调色呼吸的 Windows 控制台"**

整个界面由一个种子色生成。种子默认取 Windows 系统强调色（桌面外壳在页面加载前注入，窗口重新获得焦点时重读），用户也可以在设置里改用六个预设或自定义颜色。种子只提供 OKLCH 的色相与彩度（`--seed-h`、`--seed-c`，彩度夹在 0.04–0.16），底色、表面、描边、文字、主色、主色容器全部按固定的亮度与彩度比例从它推出；frontmatter 里的数值是 CSS 默认种子（色相 262、彩度 0.14、色调强度 1）下的浅色解析值，真正的规范是 `ui/src/app.css` 里的公式。

气质参照 Windows 11 原生控件、shadcn/ui 的精度与克制、FlClash 的莫奈取色与仪表盘板块：1px 描边的不透明板块平铺在略带色调的底色上，没有卡片阴影；信息密度偏高，数字一律用等宽表格数字。概览是 12 列便当网格，板块大小按重要性不同，拒绝“灰底加一个蓝色强调色、整齐等大卡片”的类别默认。

在 Windows 11 上窗口是 Mica 材质：页面底色与导航栏变半透明，透出系统材质；板块保持不透明，保证文字对比度。浅色与深色各一套，跟随系统。

**Key Characteristics:**
- 一个种子色生成全部界面色，状态色固定不随种子变
- 表面彩度由用户可调的色调强度（`--tint`，0–2）决定，0 为中性灰
- 12px 圆角板块、8px 圆角控件、1px 描边，板块无阴影
- 76px 左侧图标导航栏，药丸形选中指示
- Segoe UI Variable + 微软雅黑，全局 `tabular-nums`
- 签名组件是请求时间线：按结果堆叠的每分钟柱，失败带斜纹，档位切换画竖线

## Colors

单色相体系：除三种状态色外，所有颜色都是种子色相上的不同亮度与彩度。

### Primary
- **种子主色** (accent)：主按钮、复选框选中底、时间线上的档位切换竖线与标签、筛选胶囊选中文字、链接。深色主题下提亮到 0.78 并把彩度降到种子的 0.9 倍。
- **主色悬停** (accent-hover)：主按钮悬停；浅色下更深，深色下更亮。
- **主色上的文字** (on-accent)：主按钮与复选框勾的颜色。
- **主色容器** (accent-soft)：导航选中药丸、档位选中的滑动高亮、筛选胶囊选中底、选区底色、`pill.accent`。
- **主色容器上的文字** (on-accent-soft)：与 accent-soft 配对使用，不要单独用。
- **焦点环** (ring)：带 45%（深色 50%）透明度的主色，用于所有焦点与选中描边。

### Neutral
- **页面底色** (bg)：内容区背景，彩度为种子的 0.26 倍 × 色调强度。Mica 下透明度 0.55。
- **导航栏底色** (rail)：比底色略深、略浓（0.34 倍），Mica 下透明度 0.35。
- **板块表面** (surface)：板块、按钮、输入框、表头的不透明底，几乎无彩度（0.08 倍）。
- **次级表面** (surface-2)：悬停底、表格行悬停、默认胶囊与提示条底。
- **三级表面** (surface-3)：导航项悬停时的药丸底。
- **描边** (border) / **强描边** (border-strong)：所有 1px 分隔与外框；强描边用于输入框悬停、复选框边框、滚动条悬停。
- **正文 / 次要 / 弱化文字** (text / text-2 / text-3)：同色相低彩度，次要文字用于说明、表头、未选导航；弱化文字用于时间戳、坐标轴、占位符。

### 状态色（固定）
- **成功绿** (ok, 色相 155)、**降级琥珀** (warn, 色相 70)、**失败红** (bad, 色相 27)，各配一个 `-soft` 容器色，用于胶囊、提示条、日志级别、结果文字。深色主题有独立的一套亮度值。
- **时间线成功柱** (series-ok)：低彩度的种子色（0.32 倍），不用绿色，避免与状态色抢注意力；中断与失败柱分别用 warn 与 bad。

### Named Rules
**The One Seed Rule.** 界面里任何非状态色都必须写成 `oklch(L calc(var(--seed-c) * k) var(--seed-h))`；不要写死十六进制的品牌色或灰色。

**The Fixed Status Rule.** 失败红、降级琥珀、成功绿不随种子变化。种子色相与某个状态色相距不到 35° 时，根元素带 `data-seed-near`，链接改用正文色并加下划线，主色不再承担“可点击”的含义。

**The Tint Only Touches Ground Rule.** 色调强度只乘在底色、导航栏、表面与描边的彩度上；文字、主色、状态色不受它影响。

## Typography

**Display Font:** Segoe UI Variable Display（后备 Segoe UI、Microsoft YaHei UI、system-ui）
**Body Font:** Segoe UI Variable Text（后备同上）
**Label/Mono Font:** Cascadia Mono（后备 Consolas、ui-monospace）

**Character:** 系统原生字体，不引入网页字体；Display 光学尺寸只用于页面标题与时间线的总数，其余一律 Text。

### Hierarchy
- **Display** (600, 22px, 1.25, -0.01em)：每页唯一的页面标题。
- **Metric** (600, 20px)：时间线顶部的请求总数这类单个关键数字。
- **Title** (600, 14px)：板块标题；设置页的分区标题放大到 15px。
- **Body** (400, 14px, 1.5)：默认正文与控件文字；按钮为 500。
- **Body small** (13px)：提示条、筛选胶囊、表单字段名（500）、结果文字。
- **Label** (500–600, 12px)：表头、导航文字（选中时 600）、胶囊、说明小字、图例。坐标轴与时间线标签 11px。
- **Mono** (12.5px)：地址、模型 ID、日志正文、区域等可复制的技术值。

### Named Rules
**The Tabular Numbers Rule.** `font-variant-numeric: tabular-nums` 在根上全局开启；数字列一律右对齐（`td.num`）。

**The No Uppercase Rule.** 不使用全大写或加宽字距的小标签；层级靠字号与字重，不靠字距。

## Layout

窗口默认 1120px 宽，最小 820px。外壳是两列网格：76px 导航栏 + 内容区；内容区内边距上 22px、左右 28px、下 28px，自身滚动。

每页顶部一行：左边页面标题，右边是操作或状态胶囊（行间距 16px，下边距 16–18px）。设置页的这一行吸顶，铺满内容区宽度并带一条底描边，Mica 下加 24px 模糊。

概览是 12 列便当网格，间距 14px：时间线占 8 列、档位占 4 列，下一行三个 4 列板块，最后一行 12 列。窗口窄于 1080px 时 4 列板块改为 6 列、时间线占满 12 列。表格页（请求、模型）把整张表放进一个无内边距的板块；请求页窄于 900px 时隐藏档位列。设置页的分区最大宽度 920px，表单按两列或三列排布，列间距 16px。

控件间距以 4 / 8 / 10 / 12px 为主；板块内边距 16px 18px，板块标题行下边距 12px。

## Elevation & Depth

扁平，靠色调分层：导航栏比底色深一档，板块是最亮且不透明的一层，悬停用 surface-2 / surface-3。深度来自 1px 描边与表面亮度差，不来自阴影。Windows 11 上底色与导航栏是半透明的 Mica，板块浮在系统材质之上但仍不透明。

### Shadow Vocabulary
- **焦点环** (`box-shadow: 0 0 0 3px var(--ring)`)：输入框与下拉框获得焦点时，同时把描边换成主色；其余元素用 `outline: 2px solid var(--ring)`，偏移 2px。
- **选中环** (`box-shadow: 0 0 0 2px var(--ring)`)：设置页选中的种子色按钮。
- **浮层阴影** (`box-shadow: 0 4px 12px oklch(0 0 0 / 0.18)`)：时间线悬停提示这类脱离文档流的浮层。

### Named Rules
**The Flat Panel Rule.** 板块、表格、提示条永远没有阴影；阴影只给浮在内容上方的元素（提示框）和焦点 / 选中环。

**The Opaque Panel Rule.** Mica 只作用于底色与导航栏；承载文字的板块必须保持不透明 surface。

## Shapes

两档圆角：12px 给板块，8px 给按钮、输入框、提示条、档位行、提示框；复选框 5px；胶囊、筛选按钮、导航选中指示、种子色按钮用全圆角（999px）。描边统一 1px（复选框 1.5px）。时间线柱子圆角 2px，图例色块 2px。

## Components

### Buttons
- **Shape:** 8px 圆角，高 32px，左右 12px，字重 500，图标与文字间距 6px。
- **Default:** surface 底 + 1px 描边；悬停换 surface-2。
- **Primary:** 主色底与描边，on-accent 文字；悬停换 accent-hover。每个板块最多一个。
- **Ghost:** 无边无底，次要文字色；悬停出现 surface-2 底并变为正文色。图标按钮是 28px 方形 ghost。
- **Link:** 无边无底无内边距，主色文字，悬停下划线（偏移 3px）；种子近状态色时常驻下划线并改用正文色。
- **Disabled:** 透明度 0.5。过渡 0.12s。

### Chips
- **状态胶囊:** 高 22px、全圆角、12px / 600，左右 9px；默认 surface-2 底次要文字，`ok / warn / bad / accent` 变体用对应的 soft 底与实色字。可带 8px 圆点（currentColor）。
- **筛选胶囊:** 请求页与日志页的筛选按钮组，全圆角、13px、上下 4px 左右 12px；选中时 accent-soft 底、主色描边与主色文字。

### Cards / Containers
- **Corner Style:** 12px。
- **Background:** surface，不透明。
- **Shadow Strategy:** 无，见 Elevation & Depth。
- **Border:** 1px border。
- **Internal Padding:** 16px 18px；放表格时内边距为 0 并裁切溢出。
- **板块标题行:** 16px 线性图标 + 14px 标题 + 右对齐的弱化说明或链接按钮，次要文字色。

### Inputs / Fields
- **Style:** 高 34px，surface 底，1px 描边，8px 圆角，左右 10px；悬停描边换 border-strong。下拉框用内联 SVG 箭头。
- **Focus:** 描边换主色，外加 3px 焦点环。
- **Checkbox:** 自绘 18px 方框，5px 圆角，1.5px 强描边；选中为主色底，勾用 on-accent，0.15s 缩放出现。单选框与滑块用原生控件加 `accent-color: var(--accent)`。
- **Disabled:** 透明度 0.55。

### Navigation
- 76px 宽的竖向导航栏，rail 底，右侧 1px 描边，顶部 30px 应用图标。
- 每项是 20px 线性图标（描边 1.75）+ 下方 12px 文字。图标放在 52×30px 的药丸里：悬停药丸为 surface-3，选中为 accent-soft 底 on-accent-soft 图标，文字加粗到 600。药丸底色过渡 0.18s，缓动 `cubic-bezier(0.2, 0, 0, 1)`。
- 请求项在最近 10 分钟有失败时显示红色数字角标。

### Banners
- 8px 圆角、上下 10px 左右 14px、13px 文字，左侧 16px 状态图标；`bad / warn / ok` 用 soft 底与实色字，默认 surface-2。用于认证失败、端口占用、区域限制等需要就地说明的问题。

### Tables
- 表头 12px / 500 次要文字，吸顶，surface 底；单元格 9px 12px，行间 1px 描边，最后一行无描边；行悬停 surface-2。数字列右对齐。

### 档位选择器（概览）
- 单选列表，每行 8px 圆角、9px 10px 内边距、粗体名称 + 12px 说明。选中行背后是一块 accent-soft 高亮，在选项之间以 0.38s `cubic-bezier(0.16, 1, 0.3, 1)` 滑动，选中行文字变为 on-accent-soft。档位切换是一等操作，这块滑动高亮是它的反馈。

### 请求时间线（签名组件）
- 最近 30 分钟、每分钟一根柱，高 168px，柱宽为槽宽减 3px，圆角 2px。按结果堆叠：成功 series-ok、中断 warn、失败 bad 加 45° surface 色斜纹（状态不只靠颜色区分）。
- 档位降级在柱顶画一个 warn 色小三角；档位切换时刻画一条主色虚线竖线（1.5px，`4 3`）并标“切到 X”，出现时自上而下以 0.7s 画出。
- 横向网格为 `2 4` 虚线 border 色，坐标轴 11px 弱化文字。悬停时其他柱降到 0.4 透明度，显示反色提示框（text 底、bg 字、8px 圆角、浮层阴影）。
- 顶部一行：20px Display 总数 + 失败计数 + 右对齐图例。

## Do's and Don'ts

### Do:
- **Do** 从 `--seed-h` / `--seed-c` 推出所有非状态色，并让表面、描边的彩度乘上 `--tint`。
- **Do** 浅色、深色各给一套亮度；新增颜色时两套都要写。
- **Do** 用 lucide 线性图标（板块标题 16px、导航 20px / 描边 1.75），状态图标用 OctagonX（失败）、TriangleAlert（降级 / 警告）、CircleCheck（正常）。
- **Do** 让失败在颜色之外还有第二个信号（斜纹、图标、文字）。
- **Do** 普通过渡用 0.12s；有方向的移动（滑动高亮、竖线画出）用 `cubic-bezier(0.16, 1, 0.3, 1)`；并尊重 `prefers-reduced-motion`（全部过渡与动画关闭）。
- **Do** 技术值（地址、模型 ID、区域）用等宽字体，旁边给复制按钮。

### Don't:
- **Don't** 给板块、表格或提示条加阴影；层次用表面亮度与 1px 描边表达。
- **Don't** 让 Mica 透到板块里：只有底色与导航栏可以半透明。
- **Don't** 用种子色表示成功、失败或降级；状态色固定，种子近状态色时链接退回正文色。
- **Don't** 把概览做成等大卡片网格；板块按重要性占 4 / 8 / 12 列。
- **Don't** 引入网页字体或全大写加字距的小标签。
- **Don't** 画装饰性的假图表或假指标，图表只画运行时的真实请求。
