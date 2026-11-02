<script lang="ts">
  import { formatBytes, host } from "../lib/format";
  import { i18n, t } from "../lib/i18n.svelte";
  import type { ResolveState } from "../lib/types";
  import WindowsSetupPanel from "./WindowsSetupPanel.svelte";

  let { state, onretry }: { state: ResolveState; onretry: () => void } = $props();
</script>

<div class="info" aria-live="polite">
  {#if state.status === "loading"}
    <span class="spinner" aria-hidden="true"></span>
    <span class="dim">{t("resolve.loading")}</span>
  {:else if state.status === "ok"}
    {@const img = state.image}
    <dl>
      <dt>{t("resolve.latest")}</dt>
      <dd class="version">{img.version}{img.build ? ` (Build ${img.build})` : ""}</dd>
      <dt>{t("resolve.file")}</dt>
      <dd class="mono" title={img.url}>{img.filename}</dd>
      <dt>{t("resolve.size")}</dt>
      <dd>{img.size ? formatBytes(img.size, i18n.locale) : t("resolve.size_unknown")}</dd>
      <dt>{t("resolve.source")}</dt>
      <dd class="mono">{host(img.url)}</dd>
    </dl>
    {#if img.checksum.type === "https_origin"}
      <p class="full warn">⚠ {t("resolve.origin_only")}</p>
    {/if}
    {#if img.flash_mode === "windows"}
      <div class="full"><WindowsSetupPanel /></div>
    {/if}
  {:else}
    <div class="error" class:hint={state.error.code === "not_supported_yet"}>
      <strong>{t(`error.${state.error.code}`)}</strong>
      {#if state.error.detail}
        <details>
          <summary>{t("error.details")}</summary>
          <code>{state.error.detail}</code>
        </details>
      {/if}
      {#if state.error.code !== "not_supported_yet"}
        <button onclick={onretry}>{t("resolve.retry")}</button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .info {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    margin: -4px 0 0 28px;
    padding: 12px 14px;
    border-left: 2px solid var(--accent);
    background: var(--bg-elev);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    font-size: 13px;
  }
  .full {
    flex-basis: 100%;
  }
  .warn {
    margin: 0;
    font-size: 12.5px;
    color: var(--warn);
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 16px;
    margin: 0;
    min-width: 0;
  }
  dt {
    color: var(--text-faint);
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .version {
    color: var(--accent-hot);
    font-weight: 700;
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
    user-select: text;
  }
  .dim {
    color: var(--text-dim);
  }
  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .error {
    display: grid;
    gap: 6px;
    justify-items: start;
  }
  .error strong {
    color: var(--danger);
    font-weight: 600;
  }
  .error code {
    font-family: var(--mono);
    font-size: 11.5px;
    color: var(--text-dim);
    user-select: text;
    word-break: break-all;
  }
  .error.hint strong {
    color: var(--text-dim);
  }
  .error button {
    padding: 4px 12px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-elev-2);
  }
</style>
