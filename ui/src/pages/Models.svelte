<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "../lib/api";
  import { TIER_INFO } from "../lib/format";
  import { live, refreshStatus } from "../lib/live.svelte";
  import type { Models, Tier } from "../lib/types";

  let models = $state<Models | null>(null);
  // 按模型单独设的档位（Gemini 页）：没设的跟随概览里的默认档位
  const defaultTier = $derived((live.status?.gemini_tier ?? "standard") as Tier);
  const modelTiers = $derived(live.status?.gemini_model_tiers ?? {});
  let flexUnsupported = $state<string[]>([]);
  let tierSaving = $state<string | null>(null);
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

  onMount(() => {
    void load();
    void api.settings().then((s) => (flexUnsupported = s.gemini.flex_unsupported_models));
  });

  async function setModelTier(model: string, value: string) {
    tierSaving = model;
    message = null;
    try {
      await api.patchSettings({ gemini: { model_tiers: { [model]: value === "default" ? null : value } } });
      await refreshStatus();
    } catch (e) {
      message = { tone: "bad", text: errorText(e) };
    } finally {
      tierSaving = null;
    }
  }

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

<div class="segmented tabs" role="tablist">
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
      <thead>
        <tr>
          <th>客户端使用的名字</th>
          <th>Vertex 上游模型 ID</th>
          {#if tab === "gemini"}<th class="tier-col">服务档位</th>{/if}
        </tr>
      </thead>
      <tbody>
        {#each rows as [alias, upstream]}
          <tr>
            <td class="mono">{alias}</td>
            <td class="mono" class:same={alias === upstream}>{upstream}</td>
            {#if tab === "gemini"}
              {@const own = modelTiers[upstream]}
              {@const noFlex = flexUnsupported.includes(upstream)}
              <td class="tier-col">
                <select
                  class="tier-select"
                  class:own
                  value={own ?? "default"}
                  disabled={tierSaving === upstream}
                  aria-label="{alias} 的服务档位"
                  onchange={(e) => setModelTier(upstream, e.currentTarget.value)}
                >
                  <option value="default">跟随默认（{TIER_INFO[defaultTier].label}）</option>
                  {#each Object.entries(TIER_INFO) as [id, info]}
                    <option value={id} disabled={id === "flex" && noFlex}>
                      {info.label}{id === "flex" && noFlex ? "（此模型不支持）" : ""}
                    </option>
                  {/each}
                </select>
              </td>
            {/if}
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
    {#if tab === "gemini"}
      表中没有的 gemini-* 名字原样转发。服务档位按上游模型设置，对 Gemini 原生接口和 OpenAI 兼容接口都生效（后者需要档位时自动转成原生格式发送）；请求头
      <span class="mono">X-VertexSwitch-Tier</span> 可以临时覆盖。
    {/if}
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

  /* 标签用全局的分段选择样式（app.css .segmented） */
  .tabs {
    margin-bottom: 12px;
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

  td.tier-col,
  th.tier-col {
    width: 1%;
    white-space: nowrap;
  }

  td.tier-col {
    padding-top: 5px;
    padding-bottom: 5px;
  }

  .tier-select {
    width: auto;
    min-width: 168px;
    height: 30px;
    font-size: 13px;
    color: var(--text-2);
  }

  /* 单独设过的档位用主色容器标出来，一眼看出哪些模型不跟随默认 */
  .tier-select.own {
    background-color: var(--accent-soft);
    border-color: transparent;
    color: var(--on-accent-soft);
    font-weight: 600;
  }

  .hint {
    font-size: 12px;
    margin-top: 10px;
  }
</style>
