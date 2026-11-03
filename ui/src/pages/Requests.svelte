<script lang="ts">
  import { live } from "../lib/live.svelte";
  import { OUTCOME_INFO, duration, time, tokens } from "../lib/format";

  let filter = $state<"all" | "failed">("all");
  let query = $state("");

  const rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return live.requests
      .filter((r) => filter === "all" || r.outcome !== "ok")
      .filter((r) => !q || `${r.model ?? ""} ${r.path} ${r.backend ?? ""}`.toLowerCase().includes(q))
      .slice()
      .reverse();
  });
  const failed = $derived(live.requests.filter((r) => r.outcome !== "ok").length);
</script>

<header>
  <h1>请求</h1>
  <div class="tools">
    <div class="chips" role="radiogroup" aria-label="筛选">
      <button class:on={filter === "all"} onclick={() => (filter = "all")}>全部 {live.requests.length}</button>
      <button class:on={filter === "failed"} onclick={() => (filter = "failed")}>失败 {failed}</button>
    </div>
    <input type="text" placeholder="按模型或路径筛选" bind:value={query} aria-label="按模型或路径筛选" />
  </div>
</header>

<div class="card table-card">
  {#if rows.length}
    <table>
      <thead>
        <tr>
          <th>时间</th><th>入口</th><th>模型</th><th>档位</th><th>状态</th>
          <th class="num">耗时</th><th class="num">输入</th><th class="num">输出</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as r (r.seq)}
          <tr>
            <td class="mono faint">{time(r.ts_ms)}</td>
            <td>
              <span class="proto">{r.protocol}</span>{#if r.stream}<span class="pill">流式</span>{/if}
            </td>
            <td>
              <div class="mono">{r.model ?? "—"}</div>
              {#if r.error}<div class="err" title={r.error}>{r.error}</div>{/if}
            </td>
            <td>{#if r.tier}<span class="pill accent">{r.tier}</span>{:else}<span class="faint">—</span>{/if}</td>
            <td>
              <span class="pill {OUTCOME_INFO[r.outcome].tone}">
                {r.status} {OUTCOME_INFO[r.outcome].label}
              </span>
            </td>
            <td class="num">{duration(r.duration_ms)}</td>
            <td class="num">{tokens(r.input_tokens)}</td>
            <td class="num">{tokens(r.output_tokens)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <div class="empty">{live.requests.length ? "没有符合条件的请求。" : "还没有请求。客户端发来请求后会实时显示在这里。"}</div>
  {/if}
</div>
<p class="faint hint">保留最近 500 条，仅在本次运行期间。不记录请求和响应内容。</p>

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
    gap: 16px;
  }

  h1 {
    font-size: 22px;
  }

  .tools {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .tools input {
    width: 240px;
  }

  .chips {
    display: flex;
    gap: 4px;
  }

  .chips button {
    border-radius: 999px;
    padding: 4px 12px;
    font-size: 13px;
  }

  .chips button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }

  .table-card {
    padding: 0;
    overflow: hidden;
  }

  .proto {
    margin-right: 6px;
  }

  .err {
    color: var(--bad);
    font-size: 12px;
    max-width: 420px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    font-size: 12px;
    margin-top: 10px;
  }
</style>
