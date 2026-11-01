/** Bytes lesbar machen. Dezimal (GB = 10⁹), weil USB-Sticks auch so beschriftet werden. */
export function formatBytes(bytes: number, locale: string): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = bytes;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v.toLocaleString(locale, { maximumFractionDigits: i >= 3 ? 2 : 0 })} ${units[i]}`;
}

/** Nur den Hostnamen einer URL (für die Anzeige „Quelle“). */
export function host(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** Restzeit grob und menschlich: „45 s“, „3 min“, „1 h 10 min“. */
export function formatDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds < 0) return "–";
  if (seconds < 60) return `${Math.ceil(seconds)} s`;
  const min = Math.ceil(seconds / 60);
  if (min < 60) return `${min} min`;
  return `${Math.floor(min / 60)} h ${min % 60} min`;
}
