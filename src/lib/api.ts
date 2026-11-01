// Dünne, typisierte Hülle um `invoke`. So steht jeder Command-Name nur an EINER Stelle.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { Acquired, AcquireEvent, AppInfo, CatalogResponse, ResolvedImage } from "./types";

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
  appInfo: () => invoke<AppInfo>("app_info"),
};
