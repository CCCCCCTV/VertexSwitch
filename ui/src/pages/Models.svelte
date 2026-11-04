<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "../lib/api";
  import { refreshStatus } from "../lib/live.svelte";
  import type { Models } from "../lib/types";

  let models = $state<Models | null>(null);
  let tab = $state<"gemini" | "claude" | "maas" | "ollama">("gemini");
  let busy = $state(false);
  let message = $state<{ tone: "ok" | "bad"; text: string } | null>(null);

  const tabs = [
    { id: "gemini", label: "Gemini" },
    { id: "claude", label: "Claude" },
    { id: "maas", label: "开放模型（MaaS）" },
    { id: "ollama", label: "Ollama" },
  ] as const;

  const rows = $derived.by((): [string, string][] => {
    if (!models) return [];
    if (tab === "ollama") return models.ollama.map((m) => [`ollama/${m}`, m]);
    return Object.entries(models[tab]);
  });

  async function load() {
    models = await api.models();
  }

  onMount(() => void load());

  async function run(action: () => Promise<string>) {
    busy = true;
    message = null;
    try {
      message = { tone: "ok", text: await action() };
      await load();
      await refreshStatus();
    } catch (e) {
      message = { tone: "bad", text: errorText(e) };
    } finally {
      busy = false;
    }
  }

  const reload = () => run(async () => `已重新加载，共 ${(await api.reloadModels()).aliases} 个别名`);
  const edit = () => run(async () => `已打开 ${(await api.editModels()).path}；保存后自动生效`);
  const ollama = () => run(async () => `发现 ${(await api.refreshOllama()).ollama_models} 个 Ollama 模型`);
</script>

<header>
  <h1>模型</h1>
  <div class="actions">
    {#if message}<span class="msg {message.tone}">{message.text}</span>{/if}
    <button onclick={reload} disabled={busy}>重新加载</button>
    <button class="primary" onclick={edit} disabled={busy}>编辑模型表</button>
  </div>
</header>

<div class="tabs" role="tablist">
  {#each tabs as t}
    <button role="tab" aria-selected={tab === t.id} class:on={tab === t.id} onclick={() => (tab = t.id)}>
      {t.label}
      {#if models}
        <span class="count">{t.id === "ollama" ? models.ollama.length : Object.keys(models[t.id]).length}</span>
      {/if}
    </button>
  {/each}
</div>

<div class="panel table-card">
  {#if !models}
    <div class="empty">正在加载…</div>
  {:else if rows.length}
    <table>
      <thead><tr><th>客户端使用的名字</th><th>Vertex 上游模型 ID</th></tr></thead>
      <tbody>
        {#each rows as [alias, upstream]}
          <tr>
            <td class="mono">{alias}</td>
            <td class="mono" class:same={alias === upstream}>{upstream}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else if tab === "ollama"}
    <div class="empty">
      没有发现 Ollama 模型。在设置里启用 Ollama 后
      <button class="link" onclick={ollama} disabled={busy}>重新发现</button>。
    </div>
  {:else if tab === "maas"}
    <div class="empty">没有登记别名。<span class="mono">publisher/model</span> 形式的名字（如 deepseek-ai/deepseek-v3.1-maas）无需登记即可使用。</div>
  {:else}
    <div class="empty">没有模型。</div>
  {/if}
</div>

{#if models}
  <p class="faint hint">
    来源：{models.source === "builtin" ? "内置模型表" : models.source}。
    {#if tab === "gemini"}表中没有的 gemini-* 名字原样转发。{/if}
    {#if tab === "claude"}表中没有的 claude-* 名字原样转发；带日期的写法（如 -20251001）自动改为 @20251001。{/if}
    {#if models.source === "builtin"}点“编辑模型表”会把内置表复制到配置目录供修改。{/if}
  </p>
{/if}

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }

  h1 {
    font-size: 22px;
  }

  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .msg {
    font-size: 13px;
    margin-right: 6px;
  }

  .msg.ok {
    color: var(--ok);
  }

  .msg.bad {
    color: var(--bad);
  }

  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 12px;
  }

  .tabs button {
    border: none;
    background: none;
    padding: 6px 12px;
    border-radius: 7px;
    color: var(--text-2);
    font-weight: 500;
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  .count {
    margin-left: 4px;
    font-size: 12px;
    color: var(--text-3);
  }

  .table-card {
    padding: 0;
    overflow: hidden;
  }

  td.same {
    color: var(--text-3);
  }

  .hint {
    font-size: 12px;
    margin-top: 10px;
  }
</style>
