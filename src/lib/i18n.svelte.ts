// Minimale Übersetzung ohne Fremdbibliothek.
//
// - Texte liegen als flache JSON-Dateien in src/locales/<sprache>.json.
// - `t("schlüssel", { platzhalter })` liefert den Text in der aktuellen Sprache.
// - Weil `locale` ein Svelte-5-`$state` ist, aktualisiert sich die GUI automatisch,
//   wenn die Sprache umgeschaltet wird.
// - Fehlt ein Text in einer Sprache, wird Deutsch genommen; fehlt er ganz, der Schlüssel selbst
//   (so fallen fehlende Übersetzungen beim Testen sofort auf).
import de from "../locales/de.json";
import en from "../locales/en.json";
import type { LocalizedText } from "./types";

type Dict = Record<string, string>;
const dictionaries: Record<string, Dict> = { de, en };

export const languages = [
  { code: "de", label: "Deutsch" },
  { code: "en", label: "English" },
] as const;

export const i18n = $state({ locale: "de" });

export function t(key: string, params: Record<string, string | number> = {}): string {
  const text = dictionaries[i18n.locale]?.[key] ?? dictionaries.de[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? `{${name}}`));
}

/** Text aus dem Katalog in der aktuellen Sprache (gleiche Fallback-Regel wie im Rust-Code). */
export function lt(text: LocalizedText | undefined): string {
  if (!text) return "";
  return text[i18n.locale] ?? text.de ?? Object.values(text)[0] ?? "";
}
