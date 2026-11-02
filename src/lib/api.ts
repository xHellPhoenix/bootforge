// Dünne, typisierte Hülle um `invoke`. So steht jeder Command-Name nur an EINER Stelle.
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  Acquired,
  AcquireEvent,
  AppInfo,
  CacheInfo,
  CatalogResponse,
  Drive,
  ForgeEvent,
  ForgeResult,
  ForgeSource,
  LocalImage,
  WindowsSetup,
  ResolvedImage,
} from "./types";

export const api = {
  getCatalog: () => invoke<CatalogResponse>("get_catalog"),
  resolveLatest: (variantId: string, force = false) =>
    invoke<ResolvedImage>("resolve_latest", { variantId, force }),
  /** Lädt + prüft das Image. Fortschritt kommt über `onEvent` (Tauri-Channel). */
  prepareImage: (variantId: string, onEvent: (e: AcquireEvent) => void) => {
    const channel = new Channel<AcquireEvent>();
    channel.onmessage = onEvent;
    return invoke<Acquired>("prepare_image", { variantId, onEvent: channel });
  },
  cancelPrepare: () => invoke<void>("cancel_prepare"),
  /** Beschaffen + auf den Stick schreiben. `confirmName` wird im Backend erneut geprüft. */
  forge: (
    source: ForgeSource,
    drive: Drive,
    showExternalHdds: boolean,
    confirmName: string,
    windowsSetup: WindowsSetup | null,
    onEvent: (e: ForgeEvent) => void,
  ) => {
    const channel = new Channel<ForgeEvent>();
    channel.onmessage = onEvent;
    return invoke<ForgeResult>("forge", {
      request: { source, drive, showExternalHdds, confirmName, windowsSetup },
      onEvent: channel,
    });
  },
  /** `null` = Einstellungen in Ordnung, sonst Fehlermeldung. */
  validateWindowsSetup: (setup: WindowsSetup) => invoke<string | null>("validate_windows_setup", { setup }),
  /** Datei-Dialog (läuft im Backend). `null` = abgebrochen. */
  pickImage: () => invoke<string | null>("pick_image"),
  /** Öffnet die Download-Seite aus dem Katalog im Browser. */
  openDownloadPage: (variantId: string) => invoke<void>("open_download_page", { variantId }),
  /** Prüft eine eigene Datei: gegen die Hersteller-Seite (`variantId`) oder einen eingegebenen Hash. */
  verifyLocal: (path: string, check: { variantId: string } | { hash: string }, onEvent: (e: AcquireEvent) => void) => {
    const channel = new Channel<AcquireEvent>();
    channel.onmessage = onEvent;
    return invoke<LocalImage>("verify_local", {
      path,
      variantId: "variantId" in check ? check.variantId : null,
      hash: "hash" in check ? check.hash : null,
      onEvent: channel,
    });
  },
  listDrives: (showExternalHdds = false) => invoke<Drive[]>("list_drives", { showExternalHdds }),
  appInfo: () => invoke<AppInfo>("app_info"),
  cacheInfo: () => invoke<CacheInfo>("cache_info"),
  /** Liefert die freigegebenen Bytes. */
  clearCache: () => invoke<number>("clear_cache"),
};
