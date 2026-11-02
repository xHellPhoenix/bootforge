<script lang="ts">
  import { lt, t } from "../lib/i18n.svelte";
  import type { CatalogResponse, Distro, ResolveState, Variant } from "../lib/types";

  let {
    busy,
    ondownload,
    distro,
    variant,
    resolve,
    origin,
    updated,
  }: {
    /** Fortschrittsansicht offen → Knöpfe sperren. */
    busy: boolean;
    ondownload: (title: string) => void;
    distro: Distro | null;
    variant: Variant | null;
    resolve: ResolveState | undefined;
    origin: CatalogResponse["origin"];
    updated: string;
  } = $props();

  // „Ubuntu Desktop (LTS) 26.04.1“ – den Distro-Namen nicht doppeln („Windows Windows 11“).
  const label = $derived.by(() => {
    if (!variant) return "—";
    const name = lt(variant.name);
    const prefix = distro && !name.startsWith(distro.name) ? `${distro.name} ` : "";
    const version = resolve?.status === "ok" ? ` ${resolve.image.version}` : "";
    return prefix + name + version;
  });
  const ready = $derived(resolve?.status === "ok");

  // Phase 1: noch keine USB-Erkennung → Forge bleibt gesperrt.
  const usbSelected = false;
  const hint = $derived(!variant ? t("forge.hint.need_variant") : t("forge.hint.need_usb"));
</script>

<footer>
  <div class="summary">
    <div class="slot">
      <span class="label">{t("step.distro")}</span>
      <span class="value">{label}</span>
    </div>
    <div class="slot">
      <span class="label">{t("step.usb")}</span>
      <span class="value muted" title={t("usb.placeholder")}>{t("usb.none_selected")}</span>
    </div>
  </div>

  <div class="right">
    <span class="catalog">{t(`catalog.origin.${origin}`)} · {t("catalog.updated", { date: updated })}</span>
    <button class="secondary" disabled={!ready || busy} onclick={() => ondownload(label)}>
      {t("forge.download_only")}
    </button>
    <button class="forge" disabled={!ready || !usbSelected || busy} title={hint}>
      {t("forge.button")}
    </button>
  </div>
</footer>

<style>
  footer {
    display: flex;
    align-items: center;
    gap: 20px;
    padding: 12px 20px;
    border-top: 1px solid var(--border);
    background: var(--bg-elev);
  }
  .summary {
    display: flex;
    gap: 28px;
  }
  .slot {
    display: grid;
  }
  .label {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--text-faint);
  }
  .value.muted {
    color: var(--text-dim);
  }
  .right {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .catalog {
    font-size: 12px;
    color: var(--text-faint);
  }
  .secondary {
    padding: 9px 16px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-elev-2);
    font-weight: 600;
  }
  .secondary:disabled {
    opacity: 0.45;
  }
  .forge {
    padding: 10px 28px;
    border: none;
    border-radius: var(--radius-sm);
    font-weight: 700;
    font-size: 15px;
    letter-spacing: 0.3px;
    color: #1a0d00;
    background: linear-gradient(180deg, var(--accent-hot), var(--accent));
    box-shadow: 0 0 0 0 transparent;
    transition: box-shadow 0.15s;
  }
  .forge:not(:disabled):hover {
    box-shadow: 0 0 18px rgba(255, 122, 26, 0.45);
  }
  .forge:disabled {
    filter: grayscale(0.8) brightness(0.55);
  }
</style>
