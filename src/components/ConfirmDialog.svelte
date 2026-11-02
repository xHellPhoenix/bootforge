<script lang="ts">
  import { formatBytes } from "../lib/format";
  import { i18n, t } from "../lib/i18n.svelte";
  import type { Drive, WindowsSetup } from "../lib/types";

  let {
    drive,
    imageLabel,
    windowsSetup = null,
    onconfirm,
    oncancel,
  }: {
    drive: Drive;
    imageLabel: string;
    /** Wird eine Windows-Antwortdatei mitgeschrieben? Dann zur Kontrolle anzeigen. */
    windowsSetup?: WindowsSetup | null;
    onconfirm: (typedName: string) => void;
    oncancel: () => void;
  } = $props();

  let typed = $state("");
  // Gleiche Regel wie im Backend: Groß-/Kleinschreibung und Leerzeichen am Rand egal.
  const matches = $derived(typed.trim().toLowerCase() === drive.name.trim().toLowerCase());

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") oncancel();
  }
</script>

<svelte:window {onkeydown} />

<div class="backdrop" role="presentation">
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="c-title" aria-describedby="c-text">
    <div class="icon" aria-hidden="true">⚠</div>
    <h2 id="c-title">{t("confirm.title")}</h2>
    <p id="c-text">{t("confirm.text")}</p>

    <dl>
      <dt>{t("confirm.drive")}</dt>
      <dd>
        <strong>{drive.name}</strong> · {formatBytes(drive.size, i18n.locale)}
        <span class="mono">{drive.id}</span>
      </dd>
      {#if drive.mounts.length}
        <dt>{t("confirm.mounted")}</dt>
        <dd class="mono">{drive.mounts.join(", ")}</dd>
      {/if}
      <dt>{t("confirm.image")}</dt>
      <dd>{imageLabel}</dd>
      {#if windowsSetup}
        {@const w = windowsSetup}
        <dt>{t("confirm.windows_setup")}</dt>
        <dd>
          {[
            w.local_account ? t("ws.sum.account", { name: w.local_account }) : "",
            w.computer_name ? t("ws.sum.computer", { name: w.computer_name }) : "",
            w.locale ? t("ws.sum.locale", { locale: w.locale }) : "",
            w.skip_privacy ? t("ws.sum.privacy") : "",
            w.bypass_requirements ? t("ws.sum.bypass") : "",
          ]
            .filter(Boolean)
            .join(" · ")}
        </dd>
      {/if}
    </dl>

    <label>
      <span>{t("confirm.type")} <strong class="name">{drive.name}</strong></span>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        bind:value={typed}
        autofocus
        spellcheck="false"
        autocomplete="off"
        onkeydown={(e) => e.key === "Enter" && matches && onconfirm(typed)}
      />
    </label>

    <footer>
      <button class="secondary" onclick={oncancel}>{t("confirm.cancel")}</button>
      <button class="danger" disabled={!matches} onclick={() => onconfirm(typed)}>{t("confirm.go")}</button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.65);
    z-index: 20;
  }
  .dialog {
    width: min(560px, 92vw);
    padding: 26px 28px 22px;
    border-radius: 14px;
    border: 1px solid color-mix(in srgb, var(--danger) 55%, var(--border));
    background: var(--bg-elev);
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--danger) 12%, transparent), 0 20px 60px rgba(0, 0, 0, 0.5);
  }
  .icon {
    font-size: 30px;
    color: var(--danger);
    line-height: 1;
  }
  h2 {
    margin: 8px 0 6px;
    font-size: 20px;
    color: var(--danger);
  }
  p {
    margin: 0;
    color: var(--text-dim);
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 16px;
    margin: 18px 0;
    padding: 12px 14px;
    border-radius: var(--radius-sm);
    background: var(--bg);
    border: 1px solid var(--border);
  }
  dt {
    color: var(--text-faint);
    font-size: 12.5px;
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text-dim);
    margin-left: 6px;
  }
  dd.mono {
    margin-left: 0;
  }
  label {
    display: grid;
    gap: 6px;
    font-size: 13px;
  }
  .name {
    font-family: var(--mono);
    user-select: text;
  }
  input {
    padding: 9px 11px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-family: var(--mono);
  }
  input:focus {
    outline: none;
    border-color: var(--danger);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 20px;
  }
  footer button {
    padding: 9px 18px;
    border-radius: var(--radius-sm);
    font-weight: 700;
  }
  .secondary {
    border: 1px solid var(--border);
    background: var(--bg-elev-2);
  }
  .danger {
    border: none;
    color: #fff;
    background: var(--danger);
  }
  .danger:disabled {
    filter: grayscale(0.9) brightness(0.5);
  }
</style>
