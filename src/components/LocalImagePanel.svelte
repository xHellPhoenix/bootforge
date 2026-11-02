<script lang="ts">
  // Eigene Datei auswählen und prüfen.
  // - Mit `variant` (Windows): ISO auf Microsofts Seite laden, auswählen → wird automatisch gegen
  //   Microsofts veröffentlichte Prüfsummen geprüft.
  // - Ohne `variant` („Eigenes Image“): Datei auswählen + Prüfsumme vom Hersteller einfügen.
  // Erst nach erfolgreicher Prüfung gibt es das Ergebnis nach oben (`onverified`) – nur dann
  // lässt sich „Forge!“ drücken.
  import { untrack } from "svelte";
  import { api } from "../lib/api";
  import { formatBytes } from "../lib/format";
  import { i18n, t } from "../lib/i18n.svelte";
  import type { AcquireEvent, AppError, LocalImage, Variant } from "../lib/types";

  let {
    variant,
    local,
    onverified,
  }: {
    variant: Variant | null;
    local: LocalImage | null;
    onverified: (img: LocalImage | null) => void;
  } = $props();

  // Nur der Startwert (z. B. nach Zurückwechseln): danach ändert sich `path` nur durch „Datei wählen“.
  let path = $state<string | null>(untrack(() => local?.acquired.path ?? null));
  let hash = $state("");
  let verifying = $state(false);
  let progress = $state<{ done: number; total: number | null } | null>(null);
  let fetchingList = $state(false);
  let error = $state<AppError | null>(null);

  const fileName = $derived(path ? path.split(/[\\/]/).pop() : null);
  // Gleiche Regel wie im Backend (`parse_user_hash`) – hier nur, um den Knopf freizuschalten.
  const hashOk = $derived(
    /^[0-9a-f]{32}$|^[0-9a-f]{64}$|^[0-9a-f]{128}$/.test(
      hash
        .trim()
        .toLowerCase()
        .replace(/^(sha256|sha512|md5):?/, "")
        .replace(/\s+/g, ""),
    ),
  );

  async function pick() {
    error = null;
    const picked = await api.pickImage().catch(() => null);
    if (!picked) return;
    path = picked;
    onverified(null);
    // Windows: Prüfsumme kommt von Microsoft → gleich prüfen.
    if (variant) await verify();
  }

  function onEvent(e: AcquireEvent) {
    if (e.event === "stage") fetchingList = e.stage === "checksum";
    else if (e.event === "progress") progress = { done: e.done, total: e.total };
  }

  async function verify() {
    if (!path) return;
    verifying = true;
    error = null;
    progress = null;
    try {
      const check = variant ? { variantId: variant.id } : { hash };
      onverified(await api.verifyLocal(path, check, onEvent));
    } catch (e) {
      error = (e as AppError) ?? { code: "unknown" };
      onverified(null);
    } finally {
      verifying = false;
      fetchingList = false;
      progress = null;
    }
  }

  const pct = $derived(progress?.total ? Math.min(100, (progress.done / progress.total) * 100) : null);
</script>

<div class="local" class:standalone={!variant} aria-live="polite">
  {#if variant}
    <ol class="steps">
      <li>
        <span class="n">1</span>
        <div>
          <strong>{t("local.win.step1")}</strong>
          <p>{t("local.win.step1_hint")}</p>
          <button onclick={() => api.openDownloadPage(variant.id)}>{t("local.win.open_page")} ↗</button>
        </div>
      </li>
      <li>
        <span class="n">2</span>
        <div>
          <strong>{t("local.win.step2")}</strong>
          <p>{t("local.win.step2_hint")}</p>
          <button onclick={pick} disabled={verifying}>{t("local.pick")}</button>
          {#if fileName}<span class="file mono">{fileName}</span>{/if}
        </div>
      </li>
    </ol>
  {:else}
    <div class="form">
      <label for="own-file">{t("local.own.file")}</label>
      <div class="row">
        <button id="own-file" onclick={pick} disabled={verifying}>{t("local.pick")}</button>
        {#if fileName}<span class="file mono" title={path}>{fileName}</span>{/if}
      </div>
      <label for="own-hash">{t("local.own.hash")}</label>
      <input
        id="own-hash"
        class="mono"
        placeholder="sha256:…"
        spellcheck="false"
        autocomplete="off"
        bind:value={hash}
        oninput={() => onverified(null)}
        disabled={verifying}
      />
      <p class="hint">{t("local.own.hash_hint")}</p>
      <div class="row">
        <button class="primary" onclick={verify} disabled={!path || !hashOk || verifying}>{t("local.verify")}</button>
      </div>
    </div>
  {/if}

  {#if verifying}
    <div class="progress">
      <span class="label">{fetchingList ? t("local.fetching_list") : t("local.hashing")}</span>
      <div class="bar"><div class="fill" class:indeterminate={pct === null} style:width={pct === null ? "30%" : `${pct}%`}></div></div>
      {#if progress}
        <span class="sub">
          {formatBytes(progress.done, i18n.locale)}{#if progress.total} / {formatBytes(progress.total, i18n.locale)}{/if}
        </span>
      {/if}
      <button class="link" onclick={() => api.cancelPrepare()}>{t("prepare.cancel")}</button>
    </div>
  {:else if error}
    <div class="error">
      <strong>{error.code === "cancelled" ? t("error.cancelled") : t(`error.${error.code}`)}</strong>
      {#if error.detail}
        <details><summary>{t("error.details")}</summary><code>{error.detail}</code></details>
      {/if}
    </div>
  {:else if local}
    {@const a = local.acquired}
    <div class="ok">
      <strong>✓ {variant ? t("local.ok_vendor") : t("local.ok_hash")}</strong>
      <dl>
        <dt>{t("resolve.size")}</dt>
        <dd>{formatBytes(a.size, i18n.locale)}</dd>
        <dt>{a.algorithm.toUpperCase()}</dt>
        <dd class="mono">{a.hash}</dd>
        <dt>{t("local.mode")}</dt>
        <dd>{t(`distro.mode.${local.info.flash_mode}`)}</dd>
      </dl>
      {#if a.assurance === "weak"}<p class="warn">⚠ {t("prepare.assurance.weak")}</p>{/if}
      {#if local.info.boot_record === false && local.info.flash_mode === "raw"}
        <p class="warn">⚠ {t("local.no_boot_record")}</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .local {
    display: grid;
    gap: 14px;
    margin: -4px 0 0 28px;
    padding: 14px 16px;
    border-left: 2px solid var(--accent);
    background: var(--bg-elev);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    font-size: 13px;
  }
  /* „Eigenes Image“: steht allein, nicht eingerückt unter einer Variante */
  .local.standalone {
    margin: 0;
    border-left: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 14px;
  }
  .steps li {
    display: flex;
    gap: 12px;
  }
  .n {
    flex: none;
    display: grid;
    place-content: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent-hot);
    font-weight: 700;
    font-size: 12px;
  }
  p {
    margin: 2px 0 8px;
    color: var(--text-dim);
  }
  .form {
    display: grid;
    gap: 6px;
  }
  label {
    color: var(--text-faint);
    font-size: 12px;
    margin-top: 4px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  input {
    width: 100%;
    padding: 7px 9px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    font-size: 12px;
  }
  .hint {
    font-size: 12px;
    margin: 0;
  }
  button {
    padding: 6px 14px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-elev-2);
    font-weight: 600;
  }
  button.primary {
    border-color: var(--accent);
    color: var(--accent-hot);
  }
  button:disabled {
    opacity: 0.45;
  }
  button.link {
    border: none;
    background: none;
    padding: 0;
    color: var(--text-dim);
    text-decoration: underline;
    justify-self: start;
  }
  .file {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    font-family: var(--mono);
    user-select: text;
  }
  .progress {
    display: grid;
    gap: 6px;
  }
  .label {
    color: var(--text);
  }
  .sub {
    color: var(--text-dim);
    font-size: 12px;
  }
  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--bg-elev-2);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-hot));
    transition: width 0.2s;
  }
  .fill.indeterminate {
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  .error strong {
    color: var(--danger);
  }
  .error code {
    font-family: var(--mono);
    font-size: 11.5px;
    color: var(--text-dim);
    word-break: break-all;
    user-select: text;
  }
  .ok strong {
    color: var(--ok);
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 16px;
    margin: 8px 0 0;
  }
  dt {
    color: var(--text-faint);
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .warn {
    color: var(--warn);
    margin: 8px 0 0;
  }
</style>
