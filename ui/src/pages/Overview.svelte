<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "../lib/api";
  import { live, refreshStatus } from "../lib/live.svelte";
  import { TIER_INFO, duration, tokens, uptime } from "../lib/format";
  import type { Metrics, Models, TestResult, Tier } from "../lib/types";

  let { go }: { go: (page: "settings" | "requests") => void } = $props();

  let metrics = $state<Metrics | null>(null);
  let models = $state<Models | null>(null);
  let tierBusy = $state(false);
  let tierError = $state("");
  let testModel = $state("gemini-3.5-flash-lite");
  let testing = $state(false);
  let testResult = $state<TestResult | null>(null);
  let testError = $state("");
  let copied = $state("");

  const status = $derived(live.status);
  const base = $derived(status?.server.addr ? `http://${status.server.addr}` : "");
  const tierRegionNote = $derived.by(() => {
    if (!status) return "";
    const r = status.gemini_region;
    if (status.gemini_tier === "flex" && r !== "global") return `当前 Gemini 区域为 ${r}，Flex 只支持 global，请求会按 Standard 处理`;
    if (status.gemini_tier === "priority" && !["global", "us", "eu"].includes(r))
      return `当前 Gemini 区域为 ${r}，Priority 只支持 global / us / eu，请求会按 Standard 处理`;
    return "";
  });

  // 按后端汇总
  const usage = $derived.by(() => {
    const rows = new Map<string, { backend: string; requests: number; errors: number; ms: number; input: number; output: number }>();
    for (const t of metrics?.totals ?? []) {
      const row = rows.get(t.backend) ?? { backend: t.backend, requests: 0, errors: 0, ms: 0, input: 0, output: 0 };
      row.requests += t.requests;
      if (t.status >= 400) row.errors += t.requests;
      row.ms += t.duration_ms;
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

  async function loadMetrics() {
    metrics = await api.metrics();
  }

  onMount(() => {
    void loadMetrics();
    void api.models().then((m) => (models = m));
    const timer = setInterval(loadMetrics, 5000);
    return () => clearInterval(timer);
  });

  async function setTier(tier: Tier) {
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
</header>

{#if live.statusError}
  <div class="banner bad">无法连接内置核心：{live.statusError}</div>
{:else if !status}
  <div class="empty">正在加载…</div>
{:else}
  {#if status.server.error}
    <div class="banner bad spaced">
      {status.server.error}。请到 <button class="link" onclick={() => go("settings")}>设置</button> 修改监听端口。
    </div>
  {/if}

  <section class="cards">
    <div class="card stat">
      <div class="label">代理服务</div>
      {#if status.server.running}
        <div class="value"><span class="pill ok">运行中</span></div>
        <div class="sub mono">{status.server.addr}</div>
      {:else}
        <div class="value"><span class="pill bad">未监听</span></div>
        <div class="sub">{status.listen}</div>
      {/if}
    </div>
    <div class="card stat">
      <div class="label">Google 认证</div>
      {#if status.google_token === "ok"}
        <div class="value"><span class="pill ok">正常</span></div>
        <div class="sub mono" title="项目 ID">{status.project_id}</div>
      {:else}
        <div class="value"><span class="pill bad">异常</span></div>
        <div class="sub err" title={status.google_token}>{status.google_token}</div>
      {/if}
    </div>
    <div class="card stat">
      <div class="label">运行时间</div>
      <div class="value big">{uptime(status.uptime_secs)}</div>
      <div class="sub">进行中请求 {status.in_flight}</div>
    </div>
    <div class="card stat">
      <div class="label">模型</div>
      <div class="value big">{status.models.gemini + status.models.claude + status.models.maas}</div>
      <div class="sub">Gemini {status.models.gemini} · Claude {status.models.claude}{status.models.ollama ? ` · Ollama ${status.models.ollama}` : ""}</div>
    </div>
  </section>

  <section class="grid">
    <div class="card">
      <h2>Gemini 服务档位</h2>
      <div class="segmented" role="radiogroup" aria-label="Gemini 服务档位">
        {#each Object.entries(TIER_INFO) as [id, info]}
          <button
            role="radio"
            aria-checked={status.gemini_tier === id}
            class:on={status.gemini_tier === id}
            disabled={tierBusy}
            onclick={() => setTier(id as Tier)}
          >
            <strong>{info.label}</strong>
            <span>{info.hint}</span>
          </button>
        {/each}
      </div>
      <p class="note muted">
        只作用于 Gemini 原生接口（<span class="mono">/v1beta/models/…</span>）。单个请求可用请求头
        <span class="mono">X-VertexSwitch-Tier</span> 覆盖。
      </p>
      {#if tierRegionNote}<div class="banner warn">{tierRegionNote}</div>{/if}
      {#if tierError}<div class="banner bad">{tierError}</div>{/if}
    </div>

    <div class="card">
      <h2>接入地址</h2>
      {#if base}
        <div class="endpoints">
          {#each [
            ["Anthropic（Claude Code 等）", base, "ANTHROPIC_BASE_URL"],
            ["OpenAI 兼容", `${base}/v1`, "base_url"],
            ["Gemini", base, "api_endpoint"],
          ] as [name, url, hint]}
            <div class="endpoint">
              <div class="name">{name}<span class="faint mono">{hint}</span></div>
              <div class="url">
                <span class="mono">{url}</span>
                <button class="link" onclick={() => copy(url)}>{copied === url ? "已复制" : "复制"}</button>
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <p class="muted">服务未在监听。</p>
      {/if}
    </div>

    <div class="card wide">
      <div class="row-head">
        <h2>用量（本次运行）</h2>
        <button class="link" onclick={() => go("requests")}>查看请求记录 →</button>
      </div>
      {#if !metrics}
        <div class="empty">正在加载…</div>
      {:else if usage.length}
        <table>
          <thead>
            <tr>
              <th>后端</th><th class="num">请求</th><th class="num">失败</th><th class="num">平均耗时</th>
              <th class="num">输入 tokens</th><th class="num">输出 tokens</th>
            </tr>
          </thead>
          <tbody>
            {#each usage as row}
              <tr>
                <td>{row.backend === "none" ? "未路由" : row.backend}</td>
                <td class="num">{row.requests}</td>
                <td class="num">{row.errors || "—"}</td>
                <td class="num">{duration(Math.round(row.ms / row.requests))}</td>
                <td class="num">{tokens(row.input)}</td>
                <td class="num">{tokens(row.output)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <div class="empty">还没有请求。</div>
      {/if}
    </div>

    <div class="card wide">
      <h2>测试连接</h2>
      <div class="test">
        <select bind:value={testModel} aria-label="测试用模型">
          {#each testModels as m}<option value={m}>{m}</option>{/each}
        </select>
        <button class="primary" onclick={runTest} disabled={testing || !status.server.running}>
          {testing ? "请求中…" : "发送测试请求"}
        </button>
        <span class="faint">会真实调用 Vertex AI，产生少量费用（最多 32 个输出 token）</span>
      </div>
      {#if testError}
        <div class="banner bad">{testError}</div>
      {:else if testResult}
        <div class="banner" class:ok={testResult.ok} class:bad={!testResult.ok}>
          {#if testResult.ok}
            成功 · {duration(testResult.latency_ms)} · 回复：{testResult.reply}
          {:else}
            失败（HTTP {testResult.status}）：{testResult.error?.message ?? "未知错误"}
          {/if}
        </div>
      {/if}
    </div>
  </section>
{/if}

<style>
  header {
    margin-bottom: 18px;
  }

  h1 {
    font-size: 22px;
  }

  .spaced {
    margin-bottom: 16px;
  }

  .cards {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 14px;
    margin-bottom: 14px;
  }

  .stat .label {
    color: var(--text-2);
    font-size: 12.5px;
  }

  .stat .value {
    margin: 6px 0 4px;
    min-height: 26px;
    display: flex;
    align-items: center;
  }

  .stat .big {
    font-size: 20px;
    font-weight: 600;
  }

  .stat .sub {
    font-size: 12.5px;
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stat .err {
    color: var(--bad);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
  }

  .wide {
    grid-column: 1 / -1;
  }

  .segmented {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }

  .segmented button {
    display: grid;
    gap: 2px;
    text-align: left;
    padding: 10px 12px;
    border-radius: 9px;
  }

  .segmented button span {
    font-size: 12px;
    color: var(--text-2);
  }

  .segmented button.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    box-shadow: 0 0 0 1px var(--accent) inset;
  }

  .segmented button.on strong {
    color: var(--accent);
  }

  .note {
    font-size: 12.5px;
    margin: 12px 0 8px;
  }

  .endpoints {
    display: grid;
    gap: 12px;
  }

  .endpoint .name {
    display: flex;
    gap: 8px;
    align-items: baseline;
    color: var(--text-2);
    font-size: 13px;
  }

  .endpoint .name .faint {
    font-size: 11.5px;
  }

  .endpoint .url {
    display: flex;
    gap: 12px;
    align-items: center;
    margin-top: 2px;
  }

  .endpoint button {
    white-space: nowrap;
  }

  .row-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 8px;
  }

  .row-head h2 {
    font-size: 15px;
  }

  .test {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-bottom: 10px;
    flex-wrap: wrap;
  }

  .test select {
    width: 260px;
  }

  .test .faint {
    font-size: 12px;
  }

  @media (max-width: 1000px) {
    .cards {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>
