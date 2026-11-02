<script lang="ts">
  // „Windows-Einrichtung anpassen“: Fragen der Windows-Installation vorab beantworten.
  // SealFlash legt daraus eine Antwortdatei (autounattend.xml) auf den Stick.
  import { t } from "../lib/i18n.svelte";
  import { revalidate, systemLocale, winSetup } from "../lib/windowsSetup.svelte";

  // Gängige Regionen; die des eigenen PCs steht immer mit drin.
  const locales = $derived(
    [...new Set([systemLocale(), "de-DE", "de-AT", "de-CH", "en-US", "en-GB", "fr-FR", "it-IT", "es-ES", "nl-NL", "pl-PL", "tr-TR"])],
  );

  // Bei jeder Änderung prüfen lassen (kurz verzögert, nicht bei jedem Tastendruck sofort).
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    // Abhängigkeiten anfassen, damit der Effekt bei jeder Eingabe läuft.
    void [
      winSetup.enabled,
      winSetup.localAccount,
      winSetup.accountName,
      winSetup.computerName,
      winSetup.setLocale,
      winSetup.locale,
      winSetup.skipPrivacy,
      winSetup.bypassRequirements,
    ];
    clearTimeout(timer);
    timer = setTimeout(revalidate, 250);
  });
</script>

<div class="ws">
  <label class="switch">
    <input type="checkbox" bind:checked={winSetup.enabled} />
    <strong>{t("ws.title")}</strong>
  </label>
  <p class="hint">{t("ws.intro")}</p>

  {#if winSetup.enabled}
    <div class="grid">
      <label class="check">
        <input type="checkbox" bind:checked={winSetup.localAccount} />
        {t("ws.local_account")}
      </label>
      {#if winSetup.localAccount}
        <div class="sub">
          <input
            type="text"
            maxlength="20"
            placeholder={t("ws.account_name")}
            bind:value={winSetup.accountName}
            autocomplete="off"
            spellcheck="false"
          />
          <p class="hint">{t("ws.no_password")}</p>
        </div>
      {/if}

      <label class="field">
        <span>{t("ws.computer_name")}</span>
        <input
          type="text"
          maxlength="15"
          placeholder={t("ws.computer_name_placeholder")}
          bind:value={winSetup.computerName}
          autocomplete="off"
          spellcheck="false"
        />
      </label>

      <label class="check">
        <input type="checkbox" bind:checked={winSetup.setLocale} />
        {t("ws.locale")}
      </label>
      {#if winSetup.setLocale}
        <div class="sub">
          <select bind:value={winSetup.locale}>
            {#each locales as l (l)}
              <option value={l}>{l}{l === systemLocale() ? ` (${t("ws.this_pc")})` : ""}</option>
            {/each}
          </select>
        </div>
      {/if}

      <label class="check">
        <input type="checkbox" bind:checked={winSetup.skipPrivacy} />
        {t("ws.skip_privacy")}
      </label>

      <label class="check">
        <input type="checkbox" bind:checked={winSetup.bypassRequirements} />
        {t("ws.bypass")}
      </label>
      {#if winSetup.bypassRequirements}
        <p class="warn sub">⚠ {t("ws.bypass_warning")}</p>
      {/if}
    </div>

    {#if winSetup.error}
      <p class="error">{winSetup.error === "account_missing" ? t("ws.account_missing") : winSetup.error}</p>
    {/if}
    <p class="hint">{t("ws.safety")}</p>
  {/if}
</div>

<style>
  .ws {
    display: grid;
    gap: 8px;
    margin-top: 6px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }
  .switch,
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }
  .grid {
    display: grid;
    gap: 8px;
    margin-top: 4px;
  }
  .sub {
    margin-left: 24px;
  }
  .field {
    display: grid;
    gap: 4px;
  }
  .field span {
    color: var(--text-dim);
    font-size: 12px;
  }
  input[type="text"],
  select {
    width: 100%;
    max-width: 320px;
    padding: 6px 9px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-dim);
  }
  .warn {
    margin: 0;
    font-size: 12px;
    color: var(--warn);
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 12.5px;
  }
</style>
