<script lang="ts">
  import { lt, t } from "../lib/i18n.svelte";
  import type { CatalogResponse, Variant } from "../lib/types";

  let {
    variant,
    origin,
    updated,
  }: { variant: Variant | null; origin: CatalogResponse["origin"]; updated: string } = $props();

  // Phase 1: noch keine USB-Erkennung → Forge bleibt gesperrt.
  const usbSelected = false;
  const hint = $derived(!variant ? t("forge.hint.need_variant") : t("forge.hint.need_usb"));
</script>

<footer>
  <div class="summary">
    <div class="slot">
      <span class="label">{t("step.distro")}</span>
      <span class="value">{variant ? lt(variant.name) : "—"}</span>
    </div>
    <div class="slot">
      <span class="label">{t("step.usb")}</span>
      <span class="value muted" title={t("usb.placeholder")}>{t("usb.none_selected")}</span>
    </div>
  </div>

  <div class="right">
    <span class="catalog">{t(`catalog.origin.${origin}`)} · {t("catalog.updated", { date: updated })}</span>
    <button class="forge" disabled={!variant || !usbSelected} title={hint}>
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
