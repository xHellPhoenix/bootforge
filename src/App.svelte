<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./lib/api";
  import { i18n, languages, t } from "./lib/i18n.svelte";
  import type { AppError, CatalogResponse, Distro, Drive, LocalImage, ResolveState, Variant } from "./lib/types";
  import DistroList from "./components/DistroList.svelte";
  import VariantPanel, { OWN_KEY } from "./components/VariantPanel.svelte";
  import ForgeBar from "./components/ForgeBar.svelte";
  import Stepper from "./components/Stepper.svelte";
  import PreparePanel from "./components/PreparePanel.svelte";
  import UsbDialog from "./components/UsbDialog.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import { createJob, createLocalJob, run, type Job } from "./lib/job.svelte";

  let response = $state<CatalogResponse | null>(null);
  let version = $state("");
  let error = $state<AppError | null>(null);

  let selectedDistro = $state<Distro | null>(null);
  let selectedVariant = $state<Variant | null>(null);

  /** „Eigenes Image“ gewählt (statt einer Distribution aus dem Katalog). */
  let ownSelected = $state(false);

  /** Ergebnis der Versionsermittlung je Varianten-ID. */
  let resolved = $state<Record<string, ResolveState>>({});

  /** Selbst mitgebrachte, geprüfte Dateien: je Variante mit manueller Quelle bzw. `OWN_KEY`. */
  let localImages = $state<Record<string, LocalImage>>({});

  /** Laufender oder zuletzt gelaufener Download (dann wird die Fortschrittsansicht gezeigt). */
  let job = $state<Job | null>(null);

  let selectedDrive = $state<Drive | null>(null);
  let usbDialogOpen = $state(false);
  /** Mit welcher Option wurde der Stick gewählt? (Muss beim Schreiben identisch geprüft werden.) */
  let selectedWithHdds = $state(false);
  /** Offener Bestätigungsdialog: Titel des Images für die Anzeige. */
  let confirmFor = $state<string | null>(null);

  const resolvedImage = $derived.by(() => {
    const r = selectedVariant && resolved[selectedVariant.id];
    return r?.status === "ok" ? r.image : null;
  });

  /** Kommt das Image aus einer eigenen Datei? Dann unter diesem Schlüssel in `localImages`. */
  const localKey = $derived(
    ownSelected ? OWN_KEY : selectedVariant?.source.type === "manual" ? selectedVariant.id : null,
  );
  const localImage = $derived(localKey ? (localImages[localKey] ?? null) : null);
  const imageReady = $derived(localKey ? !!localImage : !!resolvedImage);
  const imageSize = $derived(localImage?.acquired.size ?? resolvedImage?.size ?? null);

  // Abgeleiteter Zustand: Schritt 1 ist erst erledigt, wenn das Image feststeht (neueste Version
  // ermittelt bzw. eigene Datei geprüft), Schritt 2, wenn ein Stick gewählt ist, der groß genug ist.
  const driveFits = $derived(!!selectedDrive && (!imageSize || selectedDrive.size >= imageSize));
  const currentStep = $derived(!imageReady ? 1 : driveFits ? 3 : 2);

  onMount(async () => {
    api
      .appInfo()
      .then((i) => (version = i.version))
      .catch(() => {});
    try {
      response = await api.getCatalog();
      const first = response.catalog.distros[0];
      if (first) pickDistro(first);
    } catch (e) {
      error = (e as AppError) ?? { code: "unknown" };
    }
  });

  function pickDistro(d: Distro) {
    ownSelected = false;
    if (selectedDistro?.id !== d.id) {
      selectedDistro = d;
      selectedVariant = null;
      if (d.variants.length === 1) pickVariant(d.variants[0]);
    }
  }

  function pickOwn() {
    ownSelected = true;
    selectedDistro = null;
    selectedVariant = null;
  }

  function setLocal(key: string, img: LocalImage | null) {
    if (img) localImages[key] = img;
    else delete localImages[key];
  }

  function pickVariant(v: Variant) {
    selectedVariant = v;
    // Manuelle Quelle (Windows): Es gibt nichts zu ermitteln – die Datei bringt der Nutzer mit.
    if (v.source.type === "manual") return;
    // Schon erfolgreich ermittelt oder gerade am Laden? Dann nicht nochmal fragen.
    const current = resolved[v.id];
    if (!current || current.status === "error") resolveVariant(v.id);
  }

  function startForge(confirmName: string) {
    const title = confirmFor;
    confirmFor = null;
    if (!selectedDrive || !title) return;
    const target = {
      drive: $state.snapshot(selectedDrive),
      showExternalHdds: selectedWithHdds,
      confirmName,
    };
    if (localKey) {
      if (!localImage) return;
      job = createLocalJob(title, $state.snapshot(localImage), target);
    } else {
      if (!resolvedImage) return;
      job = createJob(title, resolvedImage, target);
    }
    run(job);
  }

  function startDownload(title: string) {
    const r = selectedVariant && resolved[selectedVariant.id];
    if (r?.status !== "ok") return;
    job = createJob(title, r.image);
    run(job);
  }

  async function resolveVariant(id: string, force = false) {
    resolved[id] = { status: "loading" };
    try {
      resolved[id] = { status: "ok", image: await api.resolveLatest(id, force) };
    } catch (e) {
      resolved[id] = { status: "error", error: (e as AppError) ?? { code: "unknown" } };
    }
  }
</script>

<div class="shell">
  <header>
    <div class="brand">
      <svg class="logo" viewBox="0 0 64 64" aria-hidden="true">
        <use href="#bf-logo" />
      </svg>
      <div>
        <h1>SealFlash{#if version}<span class="version">v{version}</span>{/if}</h1>
        <p>{t("app.tagline")}</p>
      </div>
    </div>
    <Stepper current={currentStep} />
    <select class="lang" bind:value={i18n.locale} aria-label="Sprache / Language">
      {#each languages as l (l.code)}
        <option value={l.code}>{l.label}</option>
      {/each}
    </select>
  </header>

  {#if error}
    <div class="state error">
      <strong>{t(`error.${error.code}`)}</strong>
      {#if error.detail}
        <details><summary>{t("error.details")}</summary><code>{error.detail}</code></details>
      {/if}
    </div>
  {:else if !response}
    <div class="state">{t("catalog.loading")}</div>
  {:else}
    {#if response.origin !== "remote"}
      <div class="banner" title={response.fallback_reason ?? ""}>
        ⚠ {response.remote_rejected ? t("catalog.fallback.incompatible") : t(`catalog.fallback.${response.origin}`)}
      </div>
    {/if}
    {#if job}
      <PreparePanel {job} onclose={() => (job = null)} />
    {:else}
      <main>
        <DistroList
          catalog={response.catalog}
          selected={selectedDistro}
          onpick={pickDistro}
          own={ownSelected}
          onpickown={pickOwn}
        />
        <VariantPanel
          distro={selectedDistro}
          own={ownSelected}
          selected={selectedVariant}
          {resolved}
          {localImages}
          onpick={pickVariant}
          onretry={(id) => resolveVariant(id, true)}
          onlocal={setLocal}
        />
      </main>
    {/if}
    {#if usbDialogOpen}
      <UsbDialog
        selected={selectedDrive}
        {imageSize}
        minGb={selectedVariant?.min_usb_gb ?? null}
        onselect={(d, withHdds) => {
          selectedDrive = d;
          selectedWithHdds = withHdds;
          usbDialogOpen = false;
        }}
        onclose={(stillPresent) => {
          if (!stillPresent) selectedDrive = null;
          usbDialogOpen = false;
        }}
      />
    {/if}
    {#if confirmFor && selectedDrive}
      <ConfirmDialog
        drive={selectedDrive}
        imageLabel={confirmFor}
        onconfirm={startForge}
        oncancel={() => (confirmFor = null)}
      />
    {/if}
    <ForgeBar
      onforge={(title) => (confirmFor = title)}
      drive={selectedDrive}
      {driveFits}
      onchoosedrive={() => (usbDialogOpen = true)}
      busy={job !== null}
      ondownload={startDownload}
      distro={selectedDistro}
      variant={selectedVariant}
      resolve={selectedVariant ? resolved[selectedVariant.id] : undefined}
      own={ownSelected}
      local={localImage}
      localMode={!!localKey}
      origin={response.origin}
      updated={response.catalog.updated}
    />
  {/if}
</div>

<!-- Logo einmal definieren, überall per <use> wiederverwenden -->
<svg width="0" height="0" style="position:absolute">
  <defs>
    <linearGradient id="bf-glow" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#ffb347" />
      <stop offset="1" stop-color="#ff5e1a" />
    </linearGradient>
    <symbol id="bf-logo" viewBox="0 0 64 64">
      <path d="M8 22h40c0 6-5 10-12 10H30v8h8l4 10H14l4-10h8v-8h-6C13 32 8 28 8 22Z" fill="#c9ced8" />
      <path d="M48 22h8l-4 6h-4Z" fill="#c9ced8" />
      <path d="M33 4c2 5 8 6 6 13-1 3-4 4-7 4 2-2 2-5 0-7-1 3-3 4-5 4-2-4 2-8 6-14Z" fill="url(#bf-glow)" />
    </symbol>
  </defs>
</svg>

<style>
  .shell {
    display: grid;
    grid-template-rows: auto auto 1fr auto;
    height: 100%;
  }

  header {
    display: flex;
    align-items: center;
    gap: 24px;
    padding: 14px 20px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, var(--bg-elev) 0%, var(--bg) 100%);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 230px;
  }
  .logo {
    width: 38px;
    height: 38px;
  }
  h1 {
    margin: 0;
    font-size: 18px;
    letter-spacing: 0.3px;
  }
  .version {
    margin-left: 8px;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-faint);
  }
  .brand p {
    margin: 0;
    font-size: 12px;
    color: var(--text-dim);
  }

  .lang {
    margin-left: auto;
    background: var(--bg-elev-2);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 4px 8px;
  }

  /* Feste Zeilen: Ohne Banner bliebe Zeile 2 leer, und das Grid würde die Fußleiste dort
     hineinsetzen (automatische Platzierung). */
  .banner {
    grid-row: 2;
    padding: 8px 20px;
    font-size: 12.5px;
    color: var(--warn);
    background: color-mix(in srgb, var(--warn) 10%, var(--bg));
    border-bottom: 1px solid color-mix(in srgb, var(--warn) 30%, transparent);
  }

  main {
    grid-row: 3;
    display: grid;
    grid-template-columns: minmax(260px, 340px) 1fr;
    min-height: 0; /* damit die Spalten selbst scrollen können */
  }

  .shell > :global(footer) {
    grid-row: 4;
  }

  .state {
    grid-row: 3;
    display: grid;
    place-content: center;
    gap: 8px;
    color: var(--text-dim);
    text-align: center;
  }
  .state.error strong {
    color: var(--danger);
  }
  .state code {
    font-family: var(--mono);
    font-size: 12px;
    user-select: text;
  }
</style>
