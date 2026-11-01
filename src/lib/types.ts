// TypeScript-Gegenstück zum Rust-Datenmodell (crates/bootforge-core/src/catalog/model.rs).
// Wichtig: Änderungen am Katalogformat immer an beiden Stellen nachziehen.
// (Später kann man das mit `specta`/`ts-rs` automatisch generieren lassen.)

export type LocalizedText = Record<string, string>;

export type FlashMode = "raw" | "windows";
export type Arch = "x86_64" | "aarch64";

export interface Category {
  id: string;
  name: LocalizedText;
}

export interface Variant {
  id: string;
  name: LocalizedText;
  description: LocalizedText;
  categories: string[];
  arch: Arch;
  flash_mode: FlashMode;
  compression?: "gzip" | "xz" | "zstd";
  min_usb_gb?: number;
  source: { type: "directory_listing" | "json_api" | "github_release" | "microsoft"; [k: string]: unknown };
  checksum: { type: "file" | "from_source"; algorithm: "sha256" | "sha512" | "md5" };
  signature?: { type: "gpg_detached" | "gpg_clearsigned"; signs: "checksum_file" | "image" };
  notes?: LocalizedText;
}

export interface Distro {
  id: string;
  name: string;
  vendor?: string;
  homepage: string;
  description: LocalizedText;
  variants: Variant[];
}

export interface Catalog {
  schema_version: number;
  updated: string;
  categories: Category[];
  distros: Distro[];
}

export interface CatalogIssue {
  severity: "error" | "warning";
  location: string;
  message: string;
}

export interface CatalogResponse {
  catalog: Catalog;
  origin: "bundled" | "remote" | "cache";
  warnings: CatalogIssue[];
}

/** Fehler aus dem Rust-Backend (siehe src-tauri/src/error.rs). */
export interface AppError {
  code: string;
  detail?: string;
}

export interface AppInfo {
  version: string;
  os: string;
  arch: string;
}
