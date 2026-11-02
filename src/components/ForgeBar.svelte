<script lang="ts">
  import { lt, t } from "../lib/i18n.svelte";
  import { formatBytes } from "../lib/format";
  import { i18n } from "../lib/i18n.svelte";
  import CacheButton from "./CacheButton.svelte";
  import { winSetup } from "../lib/windowsSetup.svelte";
  import type { CatalogResponse, Distro, Drive, LocalImage, ResolveState, Variant } from "../lib/types";

  let {
    onforge,
    drive,
    driveFits,
    onchoosedrive,
    busy,
    ondownload,
    distro,
    variant,
    resolve,
    own,
    local,
    localMode,
    windowsTarget,
    origin,
    updated,
  }: {
    onforge: (title: string) => void;
    drive: Drive | null;
    driveFits: boolean;
    onchoosedrive: () => void;
    /** Fortschrittsansicht offen → Knöpfe sperren. */
    busy: boolean;
    ondownload: (title: string) => void;
    distro: Distro | null;
    variant: Variant | null;
    resolve: ResolveState | undefined;
    /** „Eigenes Image“ gewählt. */
    own: boolean;
    /** Geprüfte eigene Datei (Windows-ISO oder eigenes Image), falls gewählt. */
    local: LocalImage | null;
    /** Kommt das Image aus einer eigenen Datei (statt Download)? */
    localMode: boolean;
    /** Wird ein Windows-Installationsstick erstellt? (Dann gelten die Windows-Einstellungen.) */
    windowsTarget: boolean;
    origin: CatalogResponse["origin"];
    updated: string;
  } = $props();

  // „Ubuntu Desktop (LTS) 26.04.1“ – den Distro-Namen nicht doppeln („Windows Windows 11“).
  const label = $derived.by(() => {
    if (own) return local ? local.acquired.filename : t("local.own.title");
    if (!variant) return "—";
    const name = lt(variant.name);
    const prefix = distro && !name.startsWith(distro.name) ? `${distro.name} ` : "";
    const version = resolve?.status === "ok" ? ` ${resolve.image.version}` : "";
    return prefix + name + version;
  });
  const ready = $derived(localMode ? !!local : resolve?.status === "ok");
  // Windows-Einstellungen ungültig (z. B. Benutzername mit „?“) → erst korrigieren.
  const winInvalid = $derived(windowsTarget && winSetup.enabled && !!winSetup.error);
  const canForge = $derived(ready && !!drive && driveFits && !busy && !winInvalid);
  const hint = $derived(
    !ready
      ? localMode
        ? t("forge.hint.need_file")
        : t("forge.hint.need_variant")
      : !drive || !driveFits
        ? t("forge.hint.need_usb")
        : "",
  );
</script>

<footer>
  <div class="summary">
    <div class="slot">
      <span class="label">{t("step.distro")}</span>
      <span class="value">{label}</span>
    </div>
    <button class="slot drive" onclick={onchoosedrive} disabled={busy}>
      <span class="label">{t("step.usb")}</span>
      {#if drive}
        <span class="value" class:bad={!driveFits}>{drive.name} · {formatBytes(drive.size, i18n.locale)} ▾</span>
      {:else}
        <span class="value pick">{t("usb.choose")} ▾</span>
      {/if}
    </button>
  </div>

  <div class="right">
    <span class="catalog">{t(`catalog.origin.${origin}`)} · {t("catalog.updated", { date: updated })}</span>
    <CacheButton {busy} />
    <!-- Bei eigenen Dateien gibt es nichts herunterzuladen. -->
    <button class="secondary" disabled={!ready || busy || localMode} onclick={() => ondownload(label)}>
      {t("forge.download_only")}
    </button>
    <button class="forge" disabled={!canForge} title={hint} onclick={() => onforge(label)}>
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
  .slot.drive {
    text-align: left;
    padding: 4px 10px;
    margin: -4px -10px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: none;
  }
  .slot.drive:not(:disabled):hover {
    border-color: var(--border);
    background: var(--bg-elev-2);
  }
  .value.pick {
    color: var(--accent-hot);
  }
  .value.bad {
    color: var(--danger);
  }
  .label {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--text-faint);
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
