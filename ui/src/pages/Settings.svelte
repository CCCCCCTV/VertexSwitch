<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "../lib/api";
  import { i18n, setLangPref, t, type LangPref } from "../lib/i18n.svelte";
  import { refreshStatus } from "../lib/live.svelte";
  import {
    BACKDROPS,
    LIVE_INTERVALS,
    LIVE_SCOPES,
    PRESETS,
    TINT_DEFAULT,
    chooseBackground,
    clearBackground,
    setBackdrop,
    setBackgroundOpacity,
    setGlass,
    setLiveInterval,
    setThemePref,
    setTint,
    theme,
  } from "../lib/theme.svelte";
  import { TIERS } from "../lib/format";
  import { checkUpdate, setAutoCheck, update } from "../lib/update.svelte";
  import type { AppInfo, Backdrop, Settings } from "../lib/types";

  let original = $state<Settings | null>(null);
  let draft = $state<Settings | null>(null);
  let info = $state<AppInfo | null>(null);
  const liveScope = $derived(LIVE_SCOPES.find((s) => s === theme.pref.source));
  const LANGS: { id: LangPref; label: () => string }[] = [
    { id: "system", label: () => t("settings.langSystem") },
    { id: "zh", label: () => "中文" },
    { id: "en", label: () => "English" },
  ];
  let bgBusy = $state(false);
  let bgError = $state("");

  async function background(action: () => Promise<void>) {
    bgBusy = true;
    bgError = "";
    try {
      await action();
    } catch (e) {
      bgError = errorText(e);
    } finally {
      bgBusy = false;
    }
  }
  let saving = $state(false);
  let message = $state<{ tone: "ok" | "bad" | "warn"; text: string } | null>(null);
  let showKey = $state(false);
  let dropFields = $state("");
  let autostart = $state<boolean | null>(null);
  let systemMessage = $state<{ tone: "ok" | "bad"; text: string } | null>(null);

  /** 只包含改动字段的合并补丁（RFC 7396）。数组整体替换。 */
  function diff(a: any, b: any): any {
    const out: any = {};
    for (const key of Object.keys(b)) {
      const x = a?.[key];
      const y = b[key];
      if (y && typeof y === "object" && !Array.isArray(y)) {
        const sub = diff(x, y);
        if (Object.keys(sub).length) out[key] = sub;
      } else if (JSON.stringify(x) !== JSON.stringify(y)) {
        out[key] = y;
      }
    }
    return out;
  }

  const patch = $derived.by(() => {
    if (!original || !draft) return {};
    const next = $state.snapshot(draft) as Settings;
    next.claude.drop_body_fields = dropFields.split(",").map((s) => s.trim()).filter(Boolean);
    return diff(original, next);
  });
  const dirty = $derived(Object.keys(patch).length > 0);

  async function load() {
    const s = await api.settings();
    original = s;
    draft = structuredClone(s);
    dropFields = s.claude.drop_body_fields.join(", ");
  }

  onMount(() => {
    void load();
    void api.appInfo().then((i) => (info = i));
    void api.getAutostart().then((a) => (autostart = a.enabled)).catch(() => (autostart = null));
    return api.onSettingsChanged(() => {
      if (!dirty) void load();
    });
  });

  async function save() {
    saving = true;
    message = null;
    try {
      const result = await api.patchSettings(patch);
      original = result.settings;
      draft = structuredClone(result.settings);
      dropFields = result.settings.claude.drop_body_fields.join(", ");
      if (result.restart_error) {
        message = { tone: "bad", text: t("settings.savedBut", { error: result.restart_error }) };
      } else if (result.restart_required) {
        message = { tone: "ok", text: t("settings.savedListen", { addr: result.server?.addr ?? "" }) };
      } else {
        message = { tone: "ok", text: t("settings.saved") };
      }
      await refreshStatus();
    } catch (e) {
      message = { tone: "bad", text: errorText(e) };
    } finally {
      saving = false;
    }
  }

  function revert() {
    if (original) {
      draft = structuredClone($state.snapshot(original) as Settings);
      dropFields = original.claude.drop_body_fields.join(", ");
    }
    message = null;
  }

  async function pickKey() {
    const path = await api.pickKeyFile();
    if (path && draft) draft.google.credentials_file = path;
  }

  // 开机自启与配置文件无关，切换后立即生效
  async function toggleAutostart(enabled: boolean) {
    systemMessage = null;
    try {
      autostart = (await api.setAutostart(enabled)).enabled;
      systemMessage = { tone: "ok", text: t(enabled ? "settings.autostartOn" : "settings.autostartOff") };
    } catch (e) {
      systemMessage = { tone: "bad", text: errorText(e) };
    }
  }

  async function copyAdminToken() {
    systemMessage = null;
    try {
      await navigator.clipboard.writeText((await api.adminToken()).token);
      systemMessage = { tone: "ok", text: t("settings.tokenCopied") };
    } catch (e) {
      systemMessage = { tone: "bad", text: errorText(e) };
    }
  }

  function generateKey() {
    const bytes = crypto.getRandomValues(new Uint8Array(24));
    if (draft) draft.server.api_key = "vs-" + [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
    showKey = true;
  }
</script>

<header>
  <h1>{t("nav.settings")}</h1>
  <div class="actions">
    {#if message}<span class="msg {message.tone}">{message.text}</span>{/if}
    <button onclick={revert} disabled={!dirty || saving}>{t("settings.revert")}</button>
    <button class="primary" onclick={save} disabled={!dirty || saving}>{saving ? t("settings.saving") : t("settings.save")}</button>
  </div>
</header>

<datalist id="regions">
  <option value="global">{t("region.global")}</option>
  <option value="us">{t("region.us")}</option>
  <option value="eu">{t("region.eu")}</option>
  <option value="us-central1"></option>
  <option value="us-east5"></option>
  <option value="europe-west1"></option>
  <option value="asia-northeast1"></option>
</datalist>

{#if draft}
  <div class="sections">
    <section class="panel">
      <h2>Google Cloud</h2>
      <label class="field">
        <span>{t("settings.keyFile")}</span>
        <div class="with-btn">
          <input type="text" bind:value={draft.google.credentials_file} placeholder={t("settings.keyFilePlaceholder")}
          />
          <button onclick={pickKey}>{t("settings.browse")}</button>
        </div>
      </label>
      <div class="two">
        <label class="field">
          <span>{t("settings.projectId")}</span>
          <input type="text" bind:value={draft.google.project_id} placeholder={t("settings.projectIdPlaceholder")} />
        </label>
        <label class="field">
          <span>{t("settings.defaultRegion")}</span>
          <input type="text" list="regions" bind:value={draft.google.region} />
          <small>{t("settings.defaultRegionHint")}</small>
        </label>
      </div>
    </section>

    <section class="panel">
      <h2>Gemini</h2>
      <div class="two">
        <label class="field">
          <span>{t("settings.region")}</span>
          <input type="text" list="regions" bind:value={draft.gemini.region} placeholder={t("settings.geminiRegionPlaceholder", { region: draft.google.region })}
          />
          <small>{t("settings.geminiRegionHint")}</small>
        </label>
        <label class="field">
          <span>{t("settings.defaultTier")}</span>
          <select bind:value={draft.gemini.tier}>
            {#each TIERS as id}<option value={id}>{t(`tierOption.${id}`)}</option>{/each}
          </select>
          <small>{t("settings.defaultTierHint")}</small>
        </label>
        <label class="field">
          <span>{t("settings.openaiNative")}</span>
          <select bind:value={draft.gemini.openai_native}>
            <option value="auto">{t("settings.openaiNative.auto")}</option>
            <option value="always">{t("settings.openaiNative.always")}</option>
            <option value="never">{t("settings.openaiNative.never")}</option>
          </select>
          <small>{t("settings.openaiNativeHint")}</small>
        </label>
        <label class="field">
          <span>{t("settings.flexTimeout")}</span>
          <input type="number" min="1" max="1800" bind:value={draft.gemini.flex_server_timeout} />
          <small>{t("settings.flexTimeoutHint")}</small>
        </label>
        <label class="check">
          <input type="checkbox" bind:checked={draft.gemini.shared_only} />
          <span>{t("settings.sharedOnly")}<small>{t("settings.sharedOnlyHint")}</small></span>
        </label>
      </div>
    </section>

    <section class="panel">
      <h2>{t("settings.proxy")}</h2>
      <div class="two">
        <label class="field">
          <span>{t("settings.host")}</span>
          <input type="text" bind:value={draft.server.host} />
          <small>{t("settings.hostHint")}</small>
        </label>
        <label class="field">
          <span>{t("settings.port")}</span>
          <input type="number" min="1" max="65535" bind:value={draft.server.port} />
          <small>{t("settings.portHint")}</small>
        </label>
      </div>
      <label class="field">
        <span>API key</span>
        <div class="with-btn">
          <input type={showKey ? "text" : "password"} bind:value={draft.server.api_key} placeholder={t("settings.apiKeyPlaceholder")}
            autocomplete="off"
          />
          <button onclick={() => (showKey = !showKey)}>{showKey ? t("settings.hide") : t("settings.show")}</button>
          <button onclick={generateKey}>{t("settings.generate")}</button>
        </div>
        <small>{t("settings.apiKeyHint")}</small>
      </label>
    </section>

    <section class="panel">
      <h2>{t("settings.ollama")}</h2>
      <div class="two">
        <label class="check">
          <input type="checkbox" bind:checked={draft.ollama.enabled} />
          <span>{t("settings.enable")}<small>{t("settings.ollamaHint")}</small></span>
        </label>
        <label class="field">
          <span>{t("settings.ollamaUrl")}</span>
          <input type="text" bind:value={draft.ollama.base_url} disabled={!draft.ollama.enabled} />
        </label>
      </div>
    </section>

    <details class="panel">
      <summary><h2>{t("settings.advanced")}</h2></summary>
      <div class="three">
        <label class="field"><span>{t("settings.connectTimeout")}</span><input type="number" min="1" bind:value={draft.timeouts.connect} /></label>
        <label class="field"><span>{t("settings.readTimeout")}</span><input type="number" min="1" bind:value={draft.timeouts.read} /></label>
        <label class="field"><span>{t("settings.streamIdle")}</span><input type="number" min="1" bind:value={draft.timeouts.stream_idle} /></label>
        <label class="field"><span>{t("settings.retries")}</span><input type="number" min="0" max="5" bind:value={draft.retry.attempts} /></label>
        <label class="field">
          <span>{t("settings.quotaHang")}</span><input type="number" min="1" bind:value={draft.retry.quota_hang_seconds} />
          <small>{t("settings.quotaHangHint")}</small>
        </label>
        <label class="field"><span>{t("settings.maxTokens")}</span><input type="number" min="1" bind:value={draft.openai.default_max_tokens} /></label>
      </div>
      <label class="field">
        <span>{t("settings.dropFields")}</span>
        <input type="text" bind:value={dropFields} placeholder={t("settings.commaSeparated")} />
      </label>
    </details>

    <section class="panel" id="appearance">
      <div class="row-title"><h2>{t("settings.appearance")}</h2><span class="faint small">{t("settings.appearanceNote")}</span></div>
      <div class="seeds" role="radiogroup" aria-label={t("settings.themeColor")}>
        <button
          role="radio"
          aria-checked={theme.pref.source === "system"}
          class="seed system"
          class:on={theme.pref.source === "system"}
          onclick={() => setThemePref({ ...theme.pref, source: "system" })}
        >
          <span class="swatch" style:background={theme.systemAccent ?? "var(--accent)"}></span>
          {t("settings.followAccent")}
        </button>
        {#each PRESETS as p}
          <button
            role="radio"
            aria-checked={theme.pref.source === "custom" && theme.pref.seed.toLowerCase() === p.seed.toLowerCase()}
            aria-label={t(`preset.${p.id}`)}
            title={t(`preset.${p.id}`)}
            class="seed"
            class:on={theme.pref.source === "custom" && theme.pref.seed.toLowerCase() === p.seed.toLowerCase()}
            onclick={() => setThemePref({ ...theme.pref, source: "custom", seed: p.seed })}
          >
            <span class="swatch" style:background={p.seed}></span>
          </button>
        {/each}
        <label class="seed custom" title={t("settings.customColor")}>
          <input
            type="color"
            value={theme.seed}
            oninput={(e) => setThemePref({ ...theme.pref, source: "custom", seed: e.currentTarget.value })}
            aria-label={t("settings.customColorAria")}
          />
          {t("settings.custom")}
        </label>
      </div>
      <div class="live">
        <span class="live-label" id="live-label">{t("settings.live")}</span>
        <div class="segmented" role="radiogroup" aria-labelledby="live-label">
          <button
            role="radio"
            aria-checked={!liveScope}
            class:on={!liveScope}
            onclick={() => liveScope && setThemePref({ ...theme.pref, source: "system" })}>{t("settings.off")}</button
          >
          {#each LIVE_SCOPES as scope}
            <button
              role="radio"
              aria-checked={liveScope === scope}
              class:on={liveScope === scope}
              onclick={() => setThemePref({ ...theme.pref, source: scope })}>{t(`live.${scope}`)}</button
            >
          {/each}
        </div>
        {#if liveScope}
          <label class="interval">
            {t("settings.interval")}
            <select
              value={theme.pref.liveInterval}
              onchange={(e) => setLiveInterval(Number(e.currentTarget.value))}
            >
              {#each LIVE_INTERVALS as ms}<option value={ms}
                  >{ms ? t("interval.seconds", { n: ms / 1000 }) : t("interval.none")}</option
                >{/each}
            </select>
          </label>
        {/if}
      </div>
      <p class="faint small live-hint">
        {liveScope ? t(`live.${liveScope}.hint`) : t("live.offHint")}
        {#if liveScope && theme.pref.liveInterval === 0}
          <br />{t("live.continuous")}{liveScope === "behind" ? t("live.continuousBehind") : ""}{t("live.end")}
        {/if}
      </p>
      <label class="check inline glass-toggle">
        <input type="checkbox" checked={theme.pref.glass} onchange={(e) => setGlass(e.currentTarget.checked)} />
        <span>{t("settings.glass")}<small>{t("settings.glassHint")}</small></span>
      </label>
      <div class="tint">
        <label for="tint">{t("settings.tint")}</label>
        <span class="faint small">{t("settings.neutral")}</span>
        <input
          id="tint"
          type="range"
          min="0"
          max="2"
          step="0.05"
          value={theme.pref.tint}
          oninput={(e) => setTint(Number(e.currentTarget.value))}
        />
        <span class="faint small">{t("settings.strong")}</span>
        <button class="ghost" onclick={() => setTint(TINT_DEFAULT)} disabled={theme.pref.tint === TINT_DEFAULT}
          >{t("settings.resetDefault")}</button
        >
      </div>
      {#if theme.backdropSupported || api.preview}
        <div class="backdrop">
          <label for="backdrop">{t("settings.backdrop")}</label>
          <select
            id="backdrop"
            value={theme.pref.backdrop}
            onchange={(e) => setBackdrop(e.currentTarget.value as Backdrop)}
          >
            {#each BACKDROPS as b}<option value={b}>{t(`backdrop.${b}`)}</option>{/each}
          </select>
          <span class="faint small"
            >{t(`backdrop.${theme.pref.backdrop}.hint`)}{liveScope ? t("settings.backdropLiveNote") : ""}</span
          >
        </div>
      {/if}
      <div class="backdrop">
        <span class="row-label">{t("settings.bgImage")}</span>
        {#if theme.bgImage}
          <span class="thumb" style:background-image={`url("${theme.bgImage}")`} aria-hidden="true"></span>
        {/if}
        <button onclick={() => background(chooseBackground)} disabled={bgBusy}>
          {theme.bgImage ? t("settings.changeImage") : t("settings.chooseImage")}
        </button>
        {#if theme.bgImage}
          <button class="ghost" onclick={() => background(clearBackground)} disabled={bgBusy}
            >{t("settings.remove")}</button
          >
        {/if}
        {#if bgError}<span class="msg bad">{bgError}</span>{:else}<span class="faint small">{t("settings.bgImageHint")}</span>{/if}
      </div>
      {#if theme.bgImage}
        <div class="tint">
          <label for="bg-opacity">{t("settings.bgOpacity")}</label>
          <span class="faint small">{t("settings.faint")}</span>
          <input
            id="bg-opacity"
            type="range"
            min="0.05"
            max="1"
            step="0.05"
            value={theme.pref.bgOpacity}
            oninput={(e) => setBackgroundOpacity(Number(e.currentTarget.value))}
          />
          <span class="faint small">{t("settings.strong")}</span>
          <span class="small mono">{Math.round(theme.pref.bgOpacity * 100)}%</span>
        </div>
      {/if}
      <p class="faint small">{t("settings.themeNote")}</p>
    </section>

    <section class="panel">
      <div class="row-title">
        <h2>{t("settings.system")}</h2>
        {#if systemMessage}<span class="msg {systemMessage.tone}">{systemMessage.text}</span>{/if}
      </div>
      <div class="live lang">
        <span class="live-label" id="lang-label">{t("settings.language")}</span>
        <div class="segmented" role="radiogroup" aria-labelledby="lang-label">
          {#each LANGS as l}
            <button
              role="radio"
              aria-checked={i18n.pref === l.id}
              class:on={i18n.pref === l.id}
              lang={l.id === "system" ? undefined : l.id === "zh" ? "zh-CN" : "en"}
              onclick={() => setLangPref(l.id)}>{l.label()}</button
            >
          {/each}
        </div>
      </div>
      <label class="check inline">
        <input
          type="checkbox"
          checked={autostart ?? false}
          disabled={autostart === null}
          onchange={(e) => toggleAutostart(e.currentTarget.checked)}
        />
        <span>{t("settings.autostart")}<small>{t("settings.autostartHint")}</small></span>
      </label>
      <label class="check inline">
        <input type="checkbox" checked={update.auto} onchange={(e) => setAutoCheck(e.currentTarget.checked)} />
        <span>{t("settings.autoUpdate")}<small>{t("settings.autoUpdateHint")}</small></span>
      </label>
      {#if info}
        <dl class="facts">
          <dt>{t("settings.configFile")}</dt>
          <dd><span class="mono">{info.config_path}</span> <button class="link" onclick={() => api.openConfigDir()}
              >{t("settings.openFolder")}</button
            ></dd>
          <dt>{t("settings.adminToken")}</dt>
          <dd>
            {info.admin_token_in_store ? t("settings.credentialStore") : info.admin_token_location}
            <button class="link" onclick={copyAdminToken}>{t("settings.copy")}</button>
          </dd>
          <dt>{t("settings.version")}</dt>
          <dd>
            {info.version}
            {#if update.info?.newer && update.info.url}
              {@const url = update.info.url}
              <span class="update-new">{t("update.available", { version: update.info.latest ?? "" })}</span>
              <button class="link" onclick={() => api.openReleasePage(url)}>{t("update.download")}</button>
            {:else}
              <button class="link" disabled={update.checking} onclick={checkUpdate}
                >{update.checking ? t("update.checking") : t("update.check")}</button
              >
              {#if update.error}
                <span class="update-status update-error">{update.error}</span>
              {:else if update.info && !update.checking}
                <span class="update-status faint">{update.info.latest ? t("update.latest") : t("update.noRelease")}</span>
              {/if}
            {/if}
          </dd>
        </dl>
      {/if}
    </section>
  </div>
{:else}
  <div class="empty">{t("common.loading")}</div>
{/if}

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    /* 吸顶：铺满内容区宽度并带一条分隔线，滚动时下面的内容不会从边缘露出 */
    position: sticky;
    top: -22px;
    z-index: 2;
    margin: -22px -28px 16px;
    padding: 22px 28px 12px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
  }

  /* 半透明窗口材质下底色很薄：吸顶栏用更不透明的同色底再加模糊，滚过的内容不会透出来 */
  :global(:root[data-mica]) header {
    background: oklch(from var(--bg) l c h / 0.92);
    backdrop-filter: blur(24px) saturate(1.2);
  }

  /* 实时取色时背后是颜色场：吸顶栏半透明加模糊，颜色连贯，滚过的内容也看不清 */
  :global(:root[data-field]) header,
  :global(:root[data-glass]) header {
    background: oklch(from var(--bg) l c h / 0.35);
    backdrop-filter: blur(28px) saturate(1.2);
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

  .sections {
    display: grid;
    gap: 14px;
    max-width: 920px;
  }

  .field {
    display: grid;
    gap: 5px;
    margin-bottom: 12px;
    align-content: start;
  }

  .field > span {
    font-size: 13px;
    font-weight: 500;
  }

  small {
    font-size: 12px;
    color: var(--text-3);
  }

  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0 16px;
    align-items: start;
  }

  .three {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0 16px;
    margin-top: 12px;
    align-items: start;
  }

  .with-btn {
    display: flex;
    gap: 8px;
  }

  .with-btn input {
    flex: 1;
  }

  .check {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding-top: 24px;
    margin-bottom: 12px;
  }

  .check input {
    margin-top: 4px;
    accent-color: var(--accent);
  }

  .check span {
    display: grid;
    font-size: 13px;
    font-weight: 500;
  }

  details summary {
    cursor: pointer;
    list-style: none;
  }

  details summary h2 {
    display: inline;
    font-size: 15px;
  }

  /* 用描边画的折叠箭头，不用字符图标 */
  details summary::before {
    content: "";
    display: inline-block;
    width: 6px;
    height: 6px;
    margin: 0 10px 2px 2px;
    border: solid var(--text-3);
    border-width: 0 1.5px 1.5px 0;
    transform: rotate(-45deg);
    transition: transform 0.18s cubic-bezier(0.2, 0, 0, 1);
  }

  details[open] summary::before {
    transform: rotate(45deg);
  }

  .small {
    font-size: 12px;
  }

  .seeds {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
    margin-bottom: 10px;
  }

  .seed {
    height: 36px;
    padding: 0 10px;
    gap: 8px;
    border-radius: 999px;
  }

  .seed:not(.system):not(.custom) {
    width: 36px;
    padding: 0;
  }

  .seed.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--ring);
  }

  .swatch {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 0.12);
  }

  .seed.custom {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--border);
    background: var(--surface);
    cursor: pointer;
    font-weight: 500;
  }

  .tint {
    display: grid;
    grid-template-columns: auto auto minmax(160px, 320px) auto auto;
    justify-content: start;
    gap: 10px;
    align-items: center;
    margin-bottom: 10px;
  }

  .backdrop {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-bottom: 10px;
  }

  .backdrop label,
  .backdrop .row-label {
    font-weight: 500;
    margin-right: 6px;
  }

  .thumb {
    width: 48px;
    height: 30px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: center / cover no-repeat;
  }

  .interval {
    display: inline-flex;
    gap: 8px;
    align-items: center;
    color: var(--text-2);
    font-size: 13px;
  }

  .interval select {
    width: auto;
    height: 30px;
  }

  .backdrop select {
    width: auto;
  }

  .tint label {
    font-weight: 500;
    margin-right: 6px;
  }

  .tint input[type="range"] {
    accent-color: var(--accent);
    width: 100%;
  }

  .seed.custom input {
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    background: none;
    cursor: pointer;
  }

  .row-title {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 8px;
  }

  .row-title h2 {
    font-size: 15px;
  }

  .check.inline {
    padding-top: 0;
  }

  .glass-toggle {
    margin: 2px 0 14px;
  }

  .live {
    display: flex;
    gap: 16px;
    align-items: center;
    margin-top: 16px;
  }

  .live-label {
    font-weight: 500;
  }

  .lang {
    margin: 0 0 14px;
  }

  .live-hint {
    margin: 6px 0 14px;
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 16px;
    margin: 4px 0 0;
    font-size: 13px;
  }

  .facts dt {
    color: var(--text-2);
  }

  .facts dd {
    margin: 0;
  }

  .update-new {
    margin: 0 10px 0 8px;
    font-weight: 600;
  }

  .update-status {
    margin-left: 8px;
  }

  .update-error {
    color: var(--bad);
  }
</style>
