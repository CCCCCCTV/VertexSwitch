<script lang="ts">
  import { onMount } from "svelte";
  import {
    Activity,
    Route,
    TriangleAlert,
    OctagonX,
    CircleCheck,
    Link,
    Gauge,
    FlaskConical,
    Copy,
    Check,
    ChevronRight,
    ShieldCheck,
  } from "@lucide/svelte";
  import { api, errorText } from "../lib/api";
  import { live, refreshStatus } from "../lib/live.svelte";
  import { TIER_INFO, duration, time, tokens, uptime } from "../lib/format";
  import type { Metrics, Models, TestResult, Tier } from "../lib/types";
  import Timeline from "../components/Timeline.svelte";

  let { go }: { go: (page: "settings" | "requests" | "logs" | "models") => void } = $props();

  let metrics = $state<Metrics | null>(null);
  let models = $state<Models | null>(null);
  let tierBusy = $state(false);
  let tierError = $state("");
  let testModel = $state("gemini-3.5-flash-lite");
  let testing = $state(false);
  let testResult = $state<TestResult | null>(null);
  let testError = $state("");
  let copied = $state("");
  // 档位选中底色：一块高亮在选项之间滑动（档位切换是这个界面最重要的操作）
  let tierRows: HTMLLabelElement[] = $state([]);
  const tierIds = Object.keys(TIER_INFO) as Tier[];
  const highlight = $derived.by(() => {
    const i = tierIds.indexOf((live.status?.gemini_tier ?? "standard") as Tier);
    const row = tierRows[i];
    return row ? { top: row.offsetTop, height: row.offsetHeight } : null;
  });

  const status = $derived(live.status);
  const base = $derived(status?.server.addr ? `http://${status.server.addr}` : "");

  // 单独设了档位的模型数（在模型页设置）
  const ownTiers = $derived(Object.keys(status?.gemini_model_tiers ?? {}).length);

  // 默认档位或单独设的档位里，当前区域不支持的会按 Standard 处理
  const regionNote = $derived.by(() => {
    if (!status) return "";
    const r = status.gemini_region;
    const used = new Set([status.gemini_tier, ...Object.values(status.gemini_model_tiers ?? {})]);
    if (used.has("flex") && r !== "global")
      return `Gemini 区域是 ${r}，Flex 只支持 global，设为 Flex 的请求会按 Standard 处理。`;
    if (used.has("priority") && !["global", "us", "eu"].includes(r))
      return `Gemini 区域是 ${r}，Priority 只支持 global / us / eu，设为 Priority 的请求会按 Standard 处理。`;
    return "";
  });

  // 需要注意：最近 30 分钟的失败请求与档位降级告警
  const attention = $derived.by(() => {
    const since = Date.now() - 30 * 60_000;
    const items: { ts: number; tone: "bad" | "warn"; title: string; detail: string }[] = [];
    for (const r of live.requests) {
      if (r.ts_ms < since || r.outcome === "ok") continue;
      items.push({
        ts: r.ts_ms,
        tone: r.outcome === "error" ? "bad" : "warn",
        title: `${r.status} · ${r.model ?? r.path}`,
        detail: r.error ?? (r.outcome === "cancelled" ? "客户端提前断开" : "流式响应中断"),
      });
    }
    for (const e of live.logs) {
      if (e.ts_ms < since || e.level === "INFO") continue;
      if (!e.message.includes("实际按") && !e.message.includes("已改用 Standard")) continue;
      const [model, detail] = e.message.split("：");
      items.push({ ts: e.ts_ms, tone: "warn", title: `档位降级 · ${model}`, detail: detail ?? e.message });
    }
    return items.sort((a, b) => b.ts - a.ts).slice(0, 5);
  });

  const usage = $derived.by(() => {
    const rows = new Map<string, { backend: string; requests: number; input: number; output: number }>();
    for (const t of metrics?.totals ?? []) {
      if (t.backend === "none") continue;
      const row = rows.get(t.backend) ?? { backend: t.backend, requests: 0, input: 0, output: 0 };
      row.requests += t.requests;
      row.input += t.input_tokens;
      row.output += t.output_tokens;
      rows.set(t.backend, row);
    }
    return [...rows.values()].sort((a, b) => b.requests - a.requests);
  });

  // 健康时的基准数据：最近 30 分钟的请求数与平均耗时（都来自真实记录）
  const recent = $derived.by(() => {
    const since = Date.now() - 30 * 60_000;
    const rows = live.requests.filter((r) => r.ts_ms >= since);
    const avg = rows.length ? rows.reduce((s, r) => s + r.duration_ms, 0) / rows.length : null;
    return { count: rows.length, avg };
  });

  const totals = $derived(
    usage.reduce(
      (t, row) => ({ requests: t.requests + row.requests, input: t.input + row.input, output: t.output + row.output }),
      { requests: 0, input: 0, output: 0 },
    ),
  );

  // 测试连接：页头的一个按钮，点开是小弹层
  let testOpen = $state(false);
  let testWrap: HTMLDivElement | undefined = $state();

  function closeTestOnOutside(e: MouseEvent) {
    if (testOpen && testWrap && !testWrap.contains(e.target as Node)) testOpen = false;
  }

  const testModels = $derived([
    ...Object.keys(models?.gemini ?? {}),
    ...Object.keys(models?.claude ?? {}),
    ...Object.keys(models?.maas ?? {}),
  ]);

  onMount(() => {
    const load = async () => (metrics = await api.metrics());
    void load();
    void api.models().then((m) => (models = m));
    const timer = setInterval(load, 5000);
    return () => clearInterval(timer);
  });

  async function setTier(tier: Tier) {
    if (tier === status?.gemini_tier) return;
    tierBusy = true;
    tierError = "";
    try {
      await api.setTier(tier);
      await refreshStatus();
    } catch (e) {
      tierError = errorText(e);
    } finally {
      tierBusy = false;
    }
  }

  async function runTest() {
    testing = true;
    testResult = null;
    testError = "";
    try {
      testResult = await api.sendTest(testModel);
    } catch (e) {
      testError = errorText(e);
    } finally {
      testing = false;
    }
  }

  async function copy(text: string) {
    await navigator.clipboard.writeText(text);
    copied = text;
    setTimeout(() => (copied = ""), 1500);
  }
</script>

<svelte:window
  onclick={closeTestOnOutside}
  onkeydown={(e) => e.key === "Escape" && (testOpen = false)}
/>

<header>
  <h1>概览</h1>
  {#if status}
    <div class="chips">
      {#if status.server.running}
        <span class="pill ok"><span class="dot"></span>运行中 <span class="mono">{status.server.addr}</span></span>
      {:else}
        <span class="pill bad"><span class="dot"></span>未在监听</span>
      {/if}
      {#if status.google_token === "ok"}
        <span class="pill">Google 认证正常 · <span class="mono">{status.project_id}</span></span>
      {:else}
        <span class="pill bad" title={status.google_token}>Google 认证失败</span>
      {/if}
      <span class="pill">已运行 {uptime(status.uptime_secs)}</span>
    </div>
    <div class="test-wrap" bind:this={testWrap}>
      <button class="test-btn" aria-expanded={testOpen} aria-haspopup="dialog" onclick={() => (testOpen = !testOpen)}>
        <FlaskConical size={15} />测试连接
      </button>
      {#if testOpen}
        <div class="popover panel" role="dialog" aria-label="测试连接">
          <div class="test-row">
            <select bind:value={testModel} aria-label="测试用模型" disabled={!testModels.length}>
              {#each testModels as m}<option value={m}>{m}</option>{/each}
            </select>
            <button class="primary" onclick={runTest} disabled={testing || !status.server.running}>
              {testing ? "请求中…" : "发送"}
            </button>
          </div>
          {#if testError}
            <p class="result bad"><OctagonX size={16} />{testError}</p>
          {:else if testResult}
            <p class="result" class:ok={testResult.ok} class:bad={!testResult.ok}>
              {#if testResult.ok}
                <CircleCheck size={16} /><span>{duration(testResult.latency_ms)} · 回复：{testResult.reply}</span>
              {:else}
                <OctagonX size={16} /><span>HTTP {testResult.status}：{testResult.error?.message ?? "未知错误"}</span>
              {/if}
            </p>
          {:else}
            <p class="faint small">会真实调用 Vertex AI，产生少量费用（最多 32 个输出 token）。</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</header>

{#if live.statusError}
  <div class="banner bad spaced"><OctagonX size={16} />无法连接内置核心：{live.statusError}</div>
{:else if !status}
  <div class="empty">正在加载…</div>
{:else}
  {#if status.server.error}
    <div class="banner bad spaced">
      <OctagonX size={16} />
      <span>{status.server.error}。<button class="link" onclick={() => go("settings")}>到设置里改监听端口</button></span>
    </div>
  {/if}
  {#if status.google_token !== "ok"}
    <div class="banner bad spaced">
      <OctagonX size={16} />
      <span>Google 认证失败：{status.google_token}。<button class="link" onclick={() => go("settings")}>检查密钥文件</button></span>
    </div>
  {/if}

  <div class="bento">
    <section class="panel span-8">
      <div class="panel-head"><Activity size={16} /><h2>请求时间线</h2><span class="end faint">最近 30 分钟</span></div>
      <Timeline />
    </section>

    <section class="panel span-4">
      <div class="panel-head"><Route size={16} /><h2>Gemini 服务档位</h2></div>
      <div class="tiers" role="radiogroup" aria-label="Gemini 服务档位">
        {#if highlight}
          <span class="tier-highlight" style:transform="translateY({highlight.top}px)" style:height="{highlight.height}px"></span>
        {/if}
        {#each Object.entries(TIER_INFO) as [id, info], i}
          <label class="tier" class:on={status.gemini_tier === id} bind:this={tierRows[i]}>
            <input
              type="radio"
              name="tier"
              checked={status.gemini_tier === id}
              disabled={tierBusy}
              onchange={() => setTier(id as Tier)}
            />
            <span><b>{info.label}</b><small>{info.hint}</small></span>
          </label>
        {/each}
      </div>
      <p class="note faint">
        默认档位，Gemini 原生接口与 OpenAI 兼容接口都生效。区域：<span class="mono">{status.gemini_region}</span>
      </p>
      <p class="note own">
        <button class="more" onclick={() => go("models")}>
          {ownTiers ? `${ownTiers} 个模型单独设置了档位` : "按模型单独设置档位"}<ChevronRight size={15} />
        </button>
      </p>
      {#if regionNote}<div class="banner warn"><TriangleAlert size={16} />{regionNote}</div>{/if}
      {#if tierError}<div class="banner bad"><OctagonX size={16} />{tierError}</div>{/if}
    </section>

    <section class="panel span-4">
      <div class="panel-head">
        <TriangleAlert size={16} /><h2>需要注意</h2>
        <button class="more end" onclick={() => go("requests")}>全部请求<ChevronRight size={15} /></button>
      </div>
      {#if attention.length}
        <ul class="issues">
          {#each attention as it}
            <li class={it.tone}>
              {#if it.tone === "bad"}<OctagonX size={16} />{:else}<TriangleAlert size={16} />{/if}
              <div>
                <b>{it.title}</b>
                <small>{it.detail}</small>
              </div>
              <time class="faint">{time(it.ts).slice(0, 5)}</time>
            </li>
          {/each}
        </ul>
      {:else}
        <div class="calm">
          <span class="calm-icon"><ShieldCheck size={26} strokeWidth={1.75} /></span>
          <b>一切正常</b>
          <small>最近 30 分钟没有失败或降级</small>
        </div>
        <dl class="baseline">
          <div><dt>进行中</dt><dd>{status.in_flight}</dd></div>
          <div title="最近 30 分钟的请求数"><dt>最近请求</dt><dd>{recent.count}</dd></div>
          <div><dt>平均耗时</dt><dd>{recent.avg === null ? "—" : duration(recent.avg)}</dd></div>
        </dl>
      {/if}
    </section>

    <section class="panel span-4">
      <div class="panel-head"><Link size={16} /><h2>接入地址</h2></div>
      {#if base}
        <ul class="endpoints">
          {#each [
            ["Anthropic", base, "ANTHROPIC_BASE_URL", "Claude Code、Anthropic SDK 读这个环境变量"],
            ["OpenAI 兼容", `${base}/v1`, "OPENAI_BASE_URL", "OpenAI SDK 读这个环境变量"],
            ["Gemini", base, "GOOGLE_GEMINI_BASE_URL", "Gemini CLI、JS SDK 读这个环境变量；Python SDK 要在代码里设 http_options.base_url"],
          ] as [name, url, env, about]}
            {@const pair = `${env}=${url}`}
            <li>
              <div class="name">
                {name}
                <button class="chip" onclick={() => copy(pair)} title="点击复制 {pair}&#10;{about}">
                  {#if copied === pair}<Check size={13} />已复制{:else}<span class="mono">{env}</span>{/if}
                </button>
              </div>
              <!-- 整行是一个只读的代码框，点哪里都复制地址 -->
              <button class="url" onclick={() => copy(url)} title="点击复制" aria-label="复制 {name} 地址 {url}">
                <span class="mono">{url}</span>
                {#if copied === url}<Check size={15} />{:else}<Copy size={15} />{/if}
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="faint">服务未在监听。</p>
      {/if}
    </section>

    <section class="panel span-4">
      <div class="panel-head"><Gauge size={16} /><h2>用量</h2><span class="end faint">本次运行</span></div>
      {#if !metrics}
        <div class="empty">正在加载…</div>
      {:else}
        <!-- 三个总数始终显示（没转发过就是 0），一眼知道这里统计什么 -->
        <div class="totals">
          <div><span>请求</span><b>{totals.requests}</b></div>
          <div><span>输入 token</span><b>{tokens(totals.input)}</b></div>
          <div><span>输出 token</span><b>{tokens(totals.output)}</b></div>
        </div>
        {#if usage.length}
          <table class="usage">
            <thead><tr><th>后端</th><th class="num">请求</th><th class="num">输入</th><th class="num">输出</th></tr></thead>
            <tbody>
              {#each usage as row}
                <tr>
                  <td>{row.backend}</td>
                  <td class="num">{row.requests}</td>
                  <td class="num">{tokens(row.input)}</td>
                  <td class="num">{tokens(row.output)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else}
          <p class="faint small">转发过请求后，这里按后端列出明细。</p>
        {/if}
      {/if}
    </section>
  </div>
{/if}

<style>
  header {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
    margin-bottom: 18px;
  }

  .chips {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .chips .mono {
    font-size: 12px;
  }

  /* 测试连接：页头右侧的按钮，点开在它下面弹出一小块 */
  .test-wrap {
    position: relative;
    margin-left: auto;
  }

  .test-btn {
    height: 30px;
    padding: 0 12px;
    border-radius: 999px;
    font-size: 13px;
  }

  .popover {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: 10;
    width: 380px;
    padding: 14px;
    box-shadow: 0 16px 40px -12px oklch(0.2 calc(var(--seed-c) * 0.3) var(--seed-h) / 0.3);
  }

  .popover p {
    margin: 10px 0 0;
  }

  .spaced {
    margin-bottom: 14px;
  }

  .bento {
    display: grid;
    grid-template-columns: repeat(12, minmax(0, 1fr));
    gap: 14px;
  }

  .span-4 {
    grid-column: span 4;
  }

  .span-8 {
    grid-column: span 8;
  }

  .span-12 {
    grid-column: span 12;
  }

  /* 档位：单选列表，选中行用主色容器 */
  .tiers {
    position: relative;
    display: grid;
    gap: 6px;
  }

  .tier-highlight {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    border-radius: var(--radius-sm);
    background: var(--accent-soft);
    transition: transform 0.38s cubic-bezier(0.16, 1, 0.3, 1);
  }

  /* 三个档位都是同样的圆角框（未选中的只有一圈细描边），选中的由滑块填满 */
  .tier {
    position: relative;
    display: grid;
    grid-template-columns: 16px 1fr;
    gap: 10px;
    align-items: start;
    padding: 9px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition:
      background 0.12s,
      border-color 0.2s;
  }

  .tier input {
    margin-top: 3px;
  }

  .tier:hover:not(.on) {
    background: var(--hover);
  }

  .tier:active:not(.on) {
    background: var(--press);
  }

  .tier.on {
    border-color: transparent;
    color: var(--on-accent-soft);
    transition:
      color 0.2s,
      border-color 0.2s;
  }

  .tier span {
    display: grid;
  }

  .tier small {
    font-size: 12px;
    color: var(--text-2);
  }

  .tier.on small {
    color: inherit;
    opacity: 0.8;
  }

  .note {
    font-size: 12px;
    margin: 10px 0 8px;
  }

  .note.own {
    margin-top: -4px;
  }

  /* 需要注意 */
  .issues {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
  }

  .issues li {
    display: grid;
    grid-template-columns: 16px 1fr auto;
    gap: 10px;
    padding: 8px 0;
    border-top: 1px solid var(--border);
    align-items: start;
  }

  .issues li:first-child {
    border-top: none;
    padding-top: 0;
  }

  .issues li :global(svg) {
    margin-top: 2px;
  }

  .issues li.bad :global(svg) {
    color: var(--bad);
  }

  .issues li.warn :global(svg) {
    color: var(--warn);
  }

  .issues b {
    display: block;
    font-weight: 500;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .issues small {
    display: block;
    font-size: 12px;
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .issues div {
    min-width: 0;
  }

  .issues time {
    font-size: 12px;
  }

  /* 健康时：居中一枚柔和的盾牌，下面三项基准数据 */
  .calm {
    display: grid;
    justify-items: center;
    gap: 2px;
    padding: 6px 0 14px;
    text-align: center;
  }

  .calm-icon {
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    margin-bottom: 6px;
    border-radius: 50%;
    background: var(--ok-soft);
    color: var(--ok);
  }

  .calm b {
    font-size: 14px;
  }

  .calm small {
    font-size: 12px;
    color: var(--text-2);
  }

  .baseline,
  .totals {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
    margin: 0;
  }

  .baseline div,
  .totals div {
    display: grid;
    gap: 2px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: color-mix(in oklch, var(--surface-2) 70%, transparent);
  }

  .baseline dt,
  .totals span {
    font-size: 11.5px;
    color: var(--text-2);
  }

  .baseline dd,
  .totals b {
    margin: 0;
    font: 600 18px/1.2 var(--font-display);
  }

  .totals {
    margin-bottom: 12px;
  }

  /* 接入地址 */
  .endpoints {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }

  /* 每个地址一块浅色内嵌容器，名字、地址、复制按钮在同一块里，横向扫视不错位 */
  .endpoints li {
    padding: 8px 8px 8px 12px;
    border-radius: 10px;
    background: color-mix(in oklch, var(--surface-2) 70%, transparent);
    transition: background-color 0.15s;
  }

  .endpoints li:hover {
    background: color-mix(in oklch, var(--surface-2) 70%, var(--accent) 8%);
  }

  .endpoints .name {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12.5px;
    color: var(--text-2);
  }

  /* 环境变量名做成小标签：点一下复制 KEY=VALUE */
  .chip {
    height: 20px;
    padding: 0 8px;
    gap: 4px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text-2);
    font-size: 11px;
    font-weight: 500;
  }

  .chip .mono {
    font-size: 11px;
  }

  .chip:hover:not(:disabled) {
    color: var(--on-accent-soft);
    background: var(--accent-soft);
    border-color: transparent;
  }

  /* 地址行：一个只读代码框，整行可点复制 */
  .endpoints .url {
    width: 100%;
    height: 32px;
    margin-top: 6px;
    padding: 0 10px;
    justify-content: space-between;
    gap: 8px;
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text-3);
    font-weight: 400;
  }

  .endpoints .url .mono {
    color: var(--text);
    font-size: 12.5px;
  }

  /* 跳转链接与板块右缘 / 说明文字左缘对齐 */
  .panel-head .more {
    margin-right: -6px;
  }

  .note.own .more {
    margin-left: -10px;
  }

  /* 用量 */
  .usage th,
  .usage td {
    padding: 6px 4px;
    background: transparent;
  }

  .usage tbody tr:hover td {
    background: transparent;
  }

  /* 测试弹层 */
  .test-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .test-row select {
    flex: 1;
    min-width: 0;
  }

  .result {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    font-size: 13px;
    word-break: break-word;
  }

  .result :global(svg) {
    flex: none;
    margin-top: 2px;
  }

  .result.ok {
    color: var(--ok);
  }

  .result.bad {
    color: var(--bad);
  }

  .small {
    font-size: 12px;
  }

  @media (max-width: 1080px) {
    .span-4,
    .span-8 {
      grid-column: span 6;
    }

    .span-8 {
      grid-column: span 12;
    }
  }
</style>
