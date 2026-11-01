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

/** Antwort von `get_catalog` (Rust: `catalog::load::Loaded`). */
export interface CatalogResponse {
  catalog: Catalog;
  origin: "bundled" | "remote" | "cache";
  warnings: CatalogIssue[];
  /** Warum nicht der Online-Katalog genutzt wird. */
  fallback_reason: string | null;
}

export type HashAlgorithm = "sha256" | "sha512" | "md5";

/** Antwort von `resolve_latest` (Rust: `resolve::ResolvedImage`). */
export interface ResolvedImage {
  variant_id: string;
  version: string;
  build?: string;
  filename: string;
  url: string;
  size: number | null;
  checksum:
    | { type: "file"; url: string; algorithm: HashAlgorithm }
    | { type: "value"; value: string; algorithm: HashAlgorithm };
  signature?: {
    kind: "gpg_detached" | "gpg_clearsigned";
    signs: "checksum_file" | "image";
    url: string | null;
    fingerprints: string[];
    keyring_url: string | null;
  };
  compression: "none" | "gzip" | "xz" | "zstd";
  flash_mode: FlashMode;
}

/** Zustand der Versionsermittlung pro Variante in der GUI. */
export type ResolveState =
  | { status: "loading" }
  | { status: "ok"; image: ResolvedImage }
  | { status: "error"; error: AppError };

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
