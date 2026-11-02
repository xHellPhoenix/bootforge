<script lang="ts" module>
  /** Schlüssel für „Eigenes Image“ in `localImages`. */
  export const OWN_KEY = "__own";
</script>

<script lang="ts">
  import { lt, t } from "../lib/i18n.svelte";
  import type { Distro, LocalImage, ResolveState, Variant } from "../lib/types";
  import LocalImagePanel from "./LocalImagePanel.svelte";
  import ResolvedInfo from "./ResolvedInfo.svelte";

  let {
    distro,
    own,
    selected,
    resolved,
    localImages,
    onpick,
    onretry,
    onlocal,
  }: {
    distro: Distro | null;
    /** „Eigenes Image“ gewählt statt einer Distribution. */
    own: boolean;
    selected: Variant | null;
    resolved: Record<string, ResolveState>;
    /** Geprüfte eigene Dateien je Variante (bzw. `OWN_KEY`). */
    localImages: Record<string, LocalImage>;
    onpick: (v: Variant) => void;
    onretry: (variantId: string) => void;
    onlocal: (key: string, img: LocalImage | null) => void;
  } = $props();

  /** Wie gut ist die Integrität abgesichert? Wird als farbiger Chip angezeigt. */
  function integrity(v: Variant): { label: string; tone: "ok" | "warn" | "" } {
    if (v.checksum.type === "page_listing") return { label: t("distro.integrity.vendor_list"), tone: "" };
    if (v.signature) return { label: t("distro.integrity.gpg"), tone: "ok" };
    if (v.checksum.algorithm === "md5") return { label: t("distro.integrity.md5"), tone: "warn" };
    return { label: t(`distro.integrity.${v.checksum.algorithm}`), tone: "" };
  }
</script>

<section class="panel">
  {#if own}
    <div class="head">
      <h2>{t("local.own.title")}</h2>
      <p>{t("local.own.intro")}</p>
    </div>
    <h3>{t("local.own.heading")}</h3>
    <LocalImagePanel variant={null} local={localImages[OWN_KEY] ?? null} onverified={(img) => onlocal(OWN_KEY, img)} />
  {:else if distro}
    <div class="head">
      <h2>{distro.name}</h2>
      {#if distro.vendor}<span class="vendor">{distro.vendor}</span>{/if}
      <p>{lt(distro.description)}</p>
    </div>

    <h3>{t("distro.choose_variant")}</h3>
    <div class="variants" role="radiogroup">
      {#each distro.variants as v (v.id)}
        {@const ig = integrity(v)}
        <button
          class="variant"
          class:selected={selected?.id === v.id}
          role="radio"
          aria-checked={selected?.id === v.id}
          onclick={() => onpick(v)}
        >
          <span class="radio" aria-hidden="true"></span>
          <span class="body">
            <strong>{lt(v.name)}</strong>
            <span class="desc">{lt(v.description)}</span>
            <span class="chips">
              <span class="chip">{v.arch}</span>
              <span class="chip">{t(`distro.mode.${v.flash_mode}`)}</span>
              {#if v.min_usb_gb}<span class="chip">{t("distro.min_usb", { gb: v.min_usb_gb })}</span>{/if}
              <span class="chip {ig.tone}">{ig.label}</span>
            </span>
            {#if v.notes}<span class="note">{lt(v.notes)}</span>{/if}
          </span>
        </button>
        {#if selected?.id === v.id && v.source.type === "manual"}
          <!-- `#key`: Bei Wechsel der Variante frisch anlegen (eigener Zustand je Variante). -->
          {#key v.id}
            <LocalImagePanel variant={v} local={localImages[v.id] ?? null} onverified={(img) => onlocal(v.id, img)} />
          {/key}
        {:else if selected?.id === v.id && resolved[v.id]}
          <ResolvedInfo state={resolved[v.id]} onretry={() => onretry(v.id)} />
        {/if}
      {/each}
    </div>
  {/if}
</section>

<style>
  .panel {
    padding: 22px 26px;
    overflow-y: auto;
    min-height: 0;
  }
  .head h2 {
    display: inline;
    margin: 0 8px 0 0;
    font-size: 22px;
  }
  .vendor {
    color: var(--text-faint);
    font-size: 12px;
  }
  .head p {
    margin: 6px 0 0;
    color: var(--text-dim);
    max-width: 60ch;
  }
  h3 {
    margin: 22px 0 10px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--text-faint);
  }
  .variants {
    display: grid;
    gap: 10px;
  }
  .variant {
    display: flex;
    gap: 12px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-elev);
    text-align: left;
    transition:
      border-color 0.12s,
      background 0.12s;
  }
  .variant:hover {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
  }
  .variant.selected {
    border-color: var(--accent);
    background: linear-gradient(90deg, var(--accent-soft), var(--bg-elev) 60%);
  }
  .radio {
    flex: none;
    width: 16px;
    height: 16px;
    margin-top: 2px;
    border-radius: 50%;
    border: 2px solid var(--text-faint);
  }
  .selected .radio {
    border-color: var(--accent);
    background: radial-gradient(circle, var(--accent) 0 4px, transparent 5px);
  }
  .body {
    display: grid;
    gap: 6px;
  }
  .desc {
    color: var(--text-dim);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .note {
    font-size: 12px;
    color: var(--warn);
  }
</style>
