// Dünne, typisierte Hülle um `invoke`. So steht jeder Command-Name nur an EINER Stelle.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { Acquired, AcquireEvent, AppInfo, CatalogResponse, Drive, ForgeEvent, ForgeResult, ResolvedImage } from "./types";

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
    variantId: string,
    drive: Drive,
    showExternalHdds: boolean,
    confirmName: string,
    onEvent: (e: ForgeEvent) => void,
  ) => {
    const channel = new Channel<ForgeEvent>();
    channel.onmessage = onEvent;
    return invoke<ForgeResult>("forge", { variantId, drive, showExternalHdds, confirmName, onEvent: channel });
  },
  listDrives: (showExternalHdds = false) => invoke<Drive[]>("list_drives", { showExternalHdds }),
  appInfo: () => invoke<AppInfo>("app_info"),
};
