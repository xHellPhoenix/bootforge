<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { api } from "../lib/api";
  import { formatBytes } from "../lib/format";
  import { i18n, t } from "../lib/i18n.svelte";
  import type { AppError, Drive, Rejection } from "../lib/types";

  let {
    selected,
    imageSize,
    minGb,
    onselect,
    onclose,
  }: {
    selected: Drive | null;
    /** Größe des Images in Bytes (falls bekannt) – kleinere Sticks sind nicht wählbar. */
    imageSize: number | null;
    /** Empfohlene Mindestgröße laut Katalog (nur Hinweis). */
    minGb: number | null;
    onselect: (d: Drive) => void;
    /** `stillPresent`: Ist der bisher gewählte Stick noch eingesteckt und wählbar? */
    onclose: (stillPresent: boolean) => void;
  } = $props();

  function close() {
    // Solange noch nicht gescannt wurde, wissen wir es nicht → Auswahl lieber behalten.
    onclose(drives === null || !selected || usable.some((d) => d.id === selected.id));
  }

  let drives = $state<Drive[] | null>(null);
  let error = $state<AppError | null>(null);
  let showHdds = $state(false);
  // Bewusst nur der Startwert: Der Dialog übernimmt die bisherige Auswahl und ändert sie dann lokal,
  // erst „Auswählen“ gibt sie zurück. `untrack` sagt Svelte, dass das Absicht ist.
  let picked = $state<string | null>(untrack(() => selected?.id ?? null));
  let showHidden = $state(false);

  const usable = $derived(drives?.filter((d) => d.rejection === null) ?? []);
  const hidden = $derived(drives?.filter((d) => d.rejection !== null) ?? []);
  const pickedDrive = $derived(usable.find((d) => d.id === picked) ?? null);

  function tooSmall(d: Drive) {
    return imageSize !== null && d.size < imageSize;
  }

  function rejectionText(r: Rejection) {
    return t(`reject.${r.reason}`, { detail: "detail" in r ? String(r.detail) : "" });
  }

  async function refresh() {
    try {
      drives = await api.listDrives(showHdds);
      error = null;
      // Ausgewählter Stick abgezogen? Dann Auswahl aufheben.
      if (picked && !drives.some((d) => d.id === picked && d.rejection === null)) picked = null;
    } catch (e) {
      error = (e as AppError) ?? { code: "device_scan" };
    }
  }

  // Alle 2 s neu einlesen, solange der Dialog offen ist – so erscheinen eingesteckte Sticks von selbst.
  // (Echtes Hotplug über udev/Windows-Nachrichten wäre eleganter, ist aber pro System aufwendig;
  // ein Scan dauert nur Millisekunden.)
  let timer: ReturnType<typeof setInterval>;
  onMount(() => {
    refresh();
    timer = setInterval(refresh, 2000);
  });
  onDestroy(() => clearInterval(timer));

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }
</script>

<svelte:window {onkeydown} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="usb-title">
    <header>
      <h2 id="usb-title">{t("usb.title")}</h2>
      <p>{t("usb.intro")}</p>
    </header>

    <div class="list" role="radiogroup">
      {#if error}
        <p class="error">{t("usb.scan_error")} <code>{error.detail ?? ""}</code></p>
      {:else if drives === null}
        <p class="dim">…</p>
      {:else if usable.length === 0}
        <p class="empty">🔌 {t("usb.empty")}</p>
      {:else}
        {#each usable as d (d.id)}
          {@const small = tooSmall(d)}
          <button
            class="drive"
            class:selected={picked === d.id}
            role="radio"
            aria-checked={picked === d.id}
            disabled={small}
            onclick={() => (picked = d.id)}
          >
            <span class="radio" aria-hidden="true"></span>
            <span class="info">
              <strong>{d.name}</strong>
              <span class="meta">
                <span class="size">{formatBytes(d.size, i18n.locale)}</span>
                <span class="mono">{d.id}</span>
                {#if d.mounts.length}<span>{t("usb.mounted", { list: d.mounts.join(", ") })}</span>{/if}
              </span>
              {#if small && imageSize}
                <span class="warn bad">{t("usb.too_small", { size: formatBytes(imageSize, i18n.locale) })}</span>
              {:else if minGb && d.size < minGb * 1e9}
                <span class="warn">{t("usb.below_recommended", { gb: minGb })}</span>
              {/if}
            </span>
          </button>
        {/each}
      {/if}
    </div>

    {#if hidden.length}
      <details bind:open={showHidden} class="hidden">
        <summary>{t("usb.hidden", { n: hidden.length })}</summary>
        <ul>
          {#each hidden as d (d.id)}
            <li>
              <span class="mono">{d.id}</span>
              <span>{d.name}</span>
              <span class="dim">{formatBytes(d.size, i18n.locale)}</span>
              <span class="reason">{rejectionText(d.rejection!)}</span>
            </li>
          {/each}
        </ul>
      </details>
    {/if}

    <label class="hdd">
      <input type="checkbox" bind:checked={showHdds} onchange={refresh} />
      <span>
        {t("usb.show_hdds")}
        {#if showHdds}<small>{t("usb.show_hdds_hint")}</small>{/if}
      </span>
    </label>

    <footer>
      <button class="secondary" onclick={close}>{t("usb.close")}</button>
      <button class="primary" disabled={!pickedDrive} onclick={() => pickedDrive && onselect(pickedDrive)}>
        {t("usb.select")}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.55);
    z-index: 10;
  }
  .dialog {
    width: min(640px, 92vw);
    max-height: 86vh;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 22px 24px;
    border-radius: 14px;
    border: 1px solid var(--border);
    background: var(--bg-elev);
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  header p {
    margin: 4px 0 0;
    font-size: 12.5px;
    color: var(--text-dim);
  }
  .list {
    display: grid;
    gap: 8px;
    overflow-y: auto;
    min-height: 90px;
  }
  .drive {
    display: flex;
    gap: 12px;
    padding: 12px 14px;
    text-align: left;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--bg);
  }
  .drive:not(:disabled):hover {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
  }
  .drive.selected {
    border-color: var(--accent);
    background: linear-gradient(90deg, var(--accent-soft), var(--bg) 70%);
  }
  .drive:disabled {
    opacity: 0.6;
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
  .info {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    font-size: 12px;
    color: var(--text-dim);
  }
  .size {
    color: var(--text);
    font-weight: 600;
  }
  .mono,
  code {
    font-family: var(--mono);
    font-size: 11.5px;
  }
  .warn {
    font-size: 12px;
    color: var(--warn);
  }
  .warn.bad {
    color: var(--danger);
  }
  .empty,
  .dim {
    color: var(--text-dim);
  }
  .empty {
    padding: 18px;
    text-align: center;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
  }
  .error {
    color: var(--danger);
  }
  .hidden {
    font-size: 12.5px;
    color: var(--text-dim);
  }
  .hidden summary {
    cursor: pointer;
  }
  .hidden ul {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    display: grid;
    gap: 4px;
    max-height: 140px;
    overflow-y: auto;
  }
  .hidden li {
    display: grid;
    grid-template-columns: 120px 1fr auto;
    gap: 2px 10px;
  }
  .hidden .reason {
    grid-column: 1 / -1;
    color: var(--text-faint);
    padding-left: 130px;
    margin-top: -2px;
  }
  .hdd {
    display: flex;
    gap: 8px;
    font-size: 12.5px;
    color: var(--text-dim);
    align-items: flex-start;
  }
  .hdd small {
    display: block;
    color: var(--warn);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
  }
  footer button {
    padding: 8px 18px;
    border-radius: var(--radius-sm);
    font-weight: 600;
  }
  .secondary {
    border: 1px solid var(--border);
    background: var(--bg-elev-2);
  }
  .primary {
    border: none;
    color: #1a0d00;
    background: linear-gradient(180deg, var(--accent-hot), var(--accent));
  }
  .primary:disabled {
    filter: grayscale(0.8) brightness(0.55);
  }
</style>
