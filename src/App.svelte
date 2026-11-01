<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./lib/api";
  import { i18n, languages, t } from "./lib/i18n.svelte";
  import type { AppError, CatalogResponse, Distro, Variant } from "./lib/types";
  import DistroList from "./components/DistroList.svelte";
  import VariantPanel from "./components/VariantPanel.svelte";
  import ForgeBar from "./components/ForgeBar.svelte";
  import Stepper from "./components/Stepper.svelte";

  let response = $state<CatalogResponse | null>(null);
  let error = $state<AppError | null>(null);

  let selectedDistro = $state<Distro | null>(null);
  let selectedVariant = $state<Variant | null>(null);

  // Abgeleiteter Zustand: welcher Schritt ist gerade dran?
  const currentStep = $derived(selectedVariant ? 2 : 1);

  onMount(async () => {
    try {
      response = await api.getCatalog();
      selectedDistro = response.catalog.distros[0] ?? null;
    } catch (e) {
      error = (e as AppError) ?? { code: "unknown" };
    }
  });

  function pickDistro(d: Distro) {
    if (selectedDistro?.id !== d.id) {
      selectedDistro = d;
      selectedVariant = d.variants.length === 1 ? d.variants[0] : null;
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
        <h1>BootForge</h1>
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
    <main>
      <DistroList catalog={response.catalog} selected={selectedDistro} onpick={pickDistro} />
      <VariantPanel
        distro={selectedDistro}
        selected={selectedVariant}
        onpick={(v) => (selectedVariant = v)}
      />
    </main>
    <ForgeBar
      variant={selectedVariant}
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
    grid-template-rows: auto 1fr auto;
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

  main {
    display: grid;
    grid-template-columns: minmax(260px, 340px) 1fr;
    min-height: 0; /* damit die Spalten selbst scrollen können */
  }

  .state {
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
