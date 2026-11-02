<script lang="ts">
  // Fußleiste: Wie viel Platz belegen heruntergeladene Images? Mit „Leeren“ (zweistufig bestätigt).
  import { api } from "../lib/api";
  import { formatBytes } from "../lib/format";
  import { i18n, t } from "../lib/i18n.svelte";
  import type { AppError, CacheInfo } from "../lib/types";

  let { busy }: { busy: boolean } = $props();

  let info = $state<CacheInfo | null>(null);
  let open = $state(false);
  let confirming = $state(false);
  let error = $state<AppError | null>(null);

  async function refresh() {
    info = await api.cacheInfo().catch(() => null);
  }

  // Neu laden, sobald ein Vorgang endet (danach liegt evtl. ein neues Image im Cache).
  $effect(() => {
    if (!busy) refresh();
  });

  async function clear() {
    error = null;
    try {
      await api.clearCache();
      confirming = false;
      await refresh();
    } catch (e) {
      error = (e as AppError) ?? { code: "unknown" };
    }
  }
</script>

<div class="cache">
  <button class="chip" onclick={() => ((open = !open), (confirming = false))} aria-expanded={open}>
    {t("cache.label")}: {info ? formatBytes(info.bytes, i18n.locale) : "…"}
  </button>
  {#if open && info}
    <div class="pop" role="dialog" aria-label={t("cache.title")}>
      <strong>{t("cache.title")}</strong>
      <p>{t("cache.text", { n: info.images, size: formatBytes(info.bytes, i18n.locale) })}</p>
      {#if info.partial > 0}<p class="dim">{t("cache.partial", { n: info.partial })}</p>{/if}
      {#if info.available !== null}
        <p class="dim">{t("cache.free", { size: formatBytes(info.available, i18n.locale) })}</p>
      {/if}
      {#if error}<p class="err">{t(`error.${error.code}`)}</p>{/if}
      <div class="row">
        {#if confirming}
          <button class="danger" onclick={clear} disabled={busy}>{t("cache.confirm")}</button>
          <button onclick={() => (confirming = false)}>{t("confirm.cancel")}</button>
        {:else}
          <button onclick={() => (confirming = true)} disabled={busy || info.bytes === 0}>{t("cache.clear")}</button>
          <button onclick={() => (open = false)}>{t("usb.close")}</button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .cache {
    position: relative;
  }
  .chip {
    font-size: 12px;
    color: var(--text-faint);
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    padding: 3px 8px;
  }
  .chip:hover {
    border-color: var(--border);
    color: var(--text-dim);
  }
  .pop {
    position: absolute;
    bottom: calc(100% + 10px);
    right: 0;
    width: 300px;
    padding: 14px;
    display: grid;
    gap: 6px;
    background: var(--bg-elev-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
    z-index: 10;
    font-size: 13px;
  }
  p {
    margin: 0;
  }
  .dim {
    color: var(--text-dim);
    font-size: 12px;
  }
  .err {
    color: var(--danger);
  }
  .row {
    display: flex;
    gap: 8px;
    margin-top: 6px;
  }
  .row button {
    padding: 5px 12px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-elev);
    font-weight: 600;
  }
  .row button.danger {
    border-color: var(--danger);
    color: var(--danger);
  }
  .row button:disabled {
    opacity: 0.45;
  }
</style>
