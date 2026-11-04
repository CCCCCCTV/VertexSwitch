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
  } from "@lucide/svelte";
  import { api, errorText } from "../lib/api";
  import { live, refreshStatus } from "../lib/live.svelte";
  import { TIER_INFO, duration, time, tokens, uptime } from "../lib/format";
  import type { Metrics, Models, TestResult, Tier } from "../lib/types";
  import Timeline from "../components/Timeline.svelte";

  let { go }: { go: (page: "settings" | "requests" | "logs") => void } = $props();

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

  const regionNote = $derived.by(() => {
    if (!status) return "";
    const r = status.gemini_region;
    if (status.gemini_tier === "flex" && r !== "global")
      return `Gemini 区域是 ${r}，Flex 只支持 global，请求会按 Standard 处理。`;
    if (status.gemini_tier === "priority" && !["global", "us", "eu"].includes(r))
      return `Gemini 区域是 ${r}，Priority 只支持 global / us / eu，请求会按 Standard 处理。`;
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
        <span class="pill">Google 认证正常 · {status.project_id}</span>
      {:else}
        <span class="pill bad" title={status.google_token}>Google 认证失败</span>
      {/if}
      <span class="pill">已运行 {uptime(status.uptime_secs)}</span>
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
      <p class="note faint">只作用于 Gemini 原生接口。区域：<span class="mono">{status.gemini_region}</span></p>
      {#if regionNote}<div class="banner warn"><TriangleAlert size={16} />{regionNote}</div>{/if}
      {#if tierError}<div class="banner bad"><OctagonX size={16} />{tierError}</div>{/if}
    </section>

    <section class="panel span-4">
      <div class="panel-head">
        <TriangleAlert size={16} /><h2>需要注意</h2>
        <button class="link end" onclick={() => go("requests")}>全部请求</button>
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
        <div class="calm"><CircleCheck size={18} />最近 30 分钟没有失败或降级。</div>
      {/if}
    </section>

    <section class="panel span-4">
      <div class="panel-head"><Link size={16} /><h2>接入地址</h2></div>
      {#if base}
        <ul class="endpoints">
          {#each [
            ["Anthropic", base, "ANTHROPIC_BASE_URL"],
            ["OpenAI 兼容", `${base}/v1`, "base_url"],
            ["Gemini", base, "api_endpoint"],
          ] as [name, url, hint]}
            <li>
              <div class="name">{name}<span class="faint mono">{hint}</span></div>
              <div class="url">
                <span class="mono">{url}</span>
                <button class="ghost icon" onclick={() => copy(url)} aria-label="复制 {name} 地址">
                  {#if copied === url}<Check size={15} />{:else}<Copy size={15} />{/if}
                </button>
              </div>
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
      {:else if usage.length}
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
        <p class="faint">还没有转发过请求。</p>
      {/if}
    </section>

    <section class="panel span-12 test">
      <div class="panel-head"><FlaskConical size={16} /><h2>测试连接</h2></div>
      <div class="test-row">
        <select bind:value={testModel} aria-label="测试用模型" disabled={!testModels.length}>
          {#each testModels as m}<option value={m}>{m}</option>{/each}
        </select>
        <button class="primary" onclick={runTest} disabled={testing || !status.server.running}>
          {testing ? "请求中…" : "发送测试请求"}
        </button>
        {#if testError}
          <span class="result bad"><OctagonX size={16} />{testError}</span>
        {:else if testResult}
          <span class="result" class:ok={testResult.ok} class:bad={!testResult.ok}>
            {#if testResult.ok}
              <CircleCheck size={16} />{duration(testResult.latency_ms)} · 回复：{testResult.reply}
            {:else}
              <OctagonX size={16} />HTTP {testResult.status}：{testResult.error?.message ?? "未知错误"}
            {/if}
          </span>
        {:else}
          <span class="faint small">会真实调用 Vertex AI，产生少量费用（最多 32 个输出 token）。</span>
        {/if}
      </div>
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
    gap: 4px;
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

  .tier {
    position: relative;
    display: grid;
    grid-template-columns: 16px 1fr;
    gap: 10px;
    align-items: start;
    padding: 9px 10px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background 0.12s;
  }

  .tier input {
    margin-top: 3px;
  }

  .tier:hover:not(.on) {
    background: var(--surface-2);
  }

  .tier.on {
    color: var(--on-accent-soft);
    transition: color 0.2s;
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

  .calm {
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--text-2);
    font-size: 13px;
    padding: 6px 0;
  }

  .calm :global(svg) {
    color: var(--ok);
  }

  /* 接入地址 */
  .endpoints {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 10px;
  }

  .endpoints .name {
    display: flex;
    gap: 8px;
    align-items: baseline;
    font-size: 12.5px;
    color: var(--text-2);
  }

  .endpoints .name .faint {
    font-size: 11px;
  }

  .endpoints .url {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  button.icon {
    width: 28px;
    height: 28px;
    padding: 0;
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

  /* 测试 */
  .test .panel-head {
    margin-bottom: 10px;
  }

  .test-row {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }

  .test-row select {
    width: 260px;
  }

  .result {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    font-size: 13px;
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
