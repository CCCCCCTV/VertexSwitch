<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "../lib/api";
  import { TIERS, TIER_LABEL } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
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
    { id: "gemini", label: () => "Gemini" },
    { id: "claude", label: () => "Claude" },
    { id: "maas", label: () => t("models.maas") },
    { id: "ollama", label: () => "Ollama" },
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

  const reload = () => run(async () => t("models.reloaded", { n: (await api.reloadModels()).aliases }));
  const edit = () => run(async () => t("models.opened", { path: (await api.editModels()).path }));
  const ollama = () => run(async () => t("models.foundOllama", { n: (await api.refreshOllama()).ollama_models }));
</script>

<header>
  <h1>{t("nav.models")}</h1>
  <div class="actions">
    {#if message}<span class="msg {message.tone}">{message.text}</span>{/if}
    <button onclick={reload} disabled={busy}>{t("models.reload")}</button>
    <button class="primary" onclick={edit} disabled={busy}>{t("models.edit")}</button>
  </div>
</header>

<div class="segmented tabs" role="tablist">
  {#each tabs as item}
    <button role="tab" aria-selected={tab === item.id} class:on={tab === item.id} onclick={() => (tab = item.id)}>
      {item.label()}
      {#if models}
        <span class="count">{item.id === "ollama" ? models.ollama.length : Object.keys(models[item.id]).length}</span>
      {/if}
    </button>
  {/each}
</div>

<div class="panel table-card">
  {#if !models}
    <div class="empty">{t("common.loading")}</div>
  {:else if rows.length}
    <table>
      <thead>
        <tr>
          <th>{t("models.alias")}</th>
          <th>{t("models.upstream")}</th>
          {#if tab === "gemini"}<th class="tier-col">{t("models.tier")}</th>{/if}
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
                  aria-label={t("models.tierAria", { alias })}
                  onchange={(e) => setModelTier(upstream, e.currentTarget.value)}
                >
                  <option value="default">{t("models.followDefault", { tier: TIER_LABEL[defaultTier] })}</option>
                  {#each TIERS as id}
                    <option value={id} disabled={id === "flex" && noFlex}>
                      {TIER_LABEL[id]}{id === "flex" && noFlex ? t("models.flexUnsupported") : ""}
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
      {t("models.noOllama")}
      <button class="link" onclick={ollama} disabled={busy}>{t("models.rediscover")}</button>{t("models.noOllamaEnd")}
    </div>
  {:else if tab === "maas"}
    <div class="empty">
      {t("models.noMaas")}<span class="mono">publisher/model</span>{t("models.noMaasEnd")}
    </div>
  {:else}
    <div class="empty">{t("models.none")}</div>
  {/if}
</div>

{#if models}
  <p class="faint hint">
    {t("models.source", { source: models.source === "builtin" ? t("models.builtin") : models.source })}
    {#if tab === "gemini"}
      {t("models.geminiHint")}
      <span class="mono">X-VertexSwitch-Tier</span>
      {t("models.geminiHintEnd")}
    {/if}
    {#if tab === "claude"}{t("models.claudeHint")}{/if}
    {#if models.source === "builtin"}{t("models.builtinHint")}{/if}
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

  /* 各行一样宽：选项长短不同（如“不支持”）也不会让某一行变宽 */
  .tier-select {
    width: 100%;
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
