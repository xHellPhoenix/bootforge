// Dünne, typisierte Hülle um `invoke`. So steht jeder Command-Name nur an EINER Stelle.
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo, CatalogResponse } from "./types";

export const api = {
  getCatalog: () => invoke<CatalogResponse>("get_catalog"),
  appInfo: () => invoke<AppInfo>("app_info"),
};
