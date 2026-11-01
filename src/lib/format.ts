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
