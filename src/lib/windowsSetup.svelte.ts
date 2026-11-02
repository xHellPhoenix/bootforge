// Einstellungen für die Windows-Antwortdatei (autounattend.xml), gemeinsam für alle Komponenten.
// Gelten für den nächsten Windows-Stick; bei Linux-Images werden sie ignoriert.
import { api } from "./api";
import type { WindowsSetup } from "./types";

/** Sprache/Region dieses PCs als Vorschlag („de-DE“), falls im passenden Format. */
export function systemLocale(): string {
  const l = navigator.language;
  return /^[a-z]{2,3}-[A-Z]{2}$/.test(l) ? l : "de-DE";
}

export const winSetup = $state({
  enabled: false,
  localAccount: false,
  accountName: "",
  computerName: "",
  setLocale: true,
  locale: systemLocale(),
  skipPrivacy: true,
  bypassRequirements: false,
  /** Fehlermeldung aus dem Backend oder null. */
  error: null as string | null,
});

/** In das Format für das Backend umwandeln. `null` = keine Antwortdatei. */
export function toRequest(): WindowsSetup | null {
  if (!winSetup.enabled) return null;
  const s: WindowsSetup = {
    local_account: winSetup.localAccount ? winSetup.accountName.trim() || null : null,
    computer_name: winSetup.computerName.trim() || null,
    locale: winSetup.setLocale ? winSetup.locale : null,
    skip_privacy: winSetup.skipPrivacy,
    bypass_requirements: winSetup.bypassRequirements,
  };
  const empty = !s.local_account && !s.computer_name && !s.locale && !s.skip_privacy && !s.bypass_requirements;
  return empty ? null : s;
}

/** Eingaben vom Backend prüfen lassen (dieselben Regeln wie beim Schreiben). */
export async function revalidate() {
  const req = toRequest();
  if (winSetup.enabled && winSetup.localAccount && !winSetup.accountName.trim()) {
    winSetup.error = "account_missing";
    return;
  }
  winSetup.error = req ? await api.validateWindowsSetup(req).catch(() => null) : null;
}
