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

// --- Phase 3: Beschaffen + Prüfen ---------------------------------------------------------------

export type Stage = "checksum" | "signature" | "cache_check" | "download" | "verify" | "image_signature";

/** Schritte beim Schreiben (Rust: `flash::FlashStage`). */
export type FlashStage = "check_drive" | "verify_image" | "prepare" | "write" | "readback";

export interface Signer {
  fingerprint: string;
  user_id: string | null;
}

/** Ereignisse von `prepare_image` (Rust: `acquire::AcquireEvent`). */
export type AcquireEvent =
  | { event: "stage"; stage: Stage }
  | { event: "progress"; stage: Stage; done: number; total: number | null; bytes_per_sec: number }
  | { event: "signed"; signer: Signer };

/** Ergebnis von `prepare_image` (Rust: `acquire::Acquired`). */
export interface Acquired {
  path: string;
  filename: string;
  size: number;
  algorithm: HashAlgorithm;
  hash: string;
  signer: Signer | null;
  assurance: "signed" | "checksum" | "weak";
  from_cache: boolean;
}

// --- Phase 4: Laufwerke ---------------------------------------------------------------------------

export type Bus = "usb" | "sata" | "nvme" | "mmc" | "scsi" | "virtio" | "virtual" | "other";

/** Warum ein Laufwerk nicht wählbar ist (Rust: `device::Rejection`). */
export type Rejection =
  | { reason: "system"; detail: string }
  | { reason: "not_usb"; detail: Bus }
  | { reason: "unexpected"; detail: string }
  | { reason: "read_only" }
  | { reason: "in_use"; detail: string }
  | { reason: "no_media" }
  | { reason: "too_large" }
  | { reason: "external_hdd" };

/** Rust: `device::Drive`. */
export interface Drive {
  id: string;
  name: string;
  serial: string | null;
  size: number;
  bus: Bus;
  removable: boolean;
  mounts: string[];
  rejection: Rejection | null;
}

// --- Phase 5: Schreiben ---------------------------------------------------------------------------

export type FlashEvent =
  | { event: "stage"; stage: FlashStage }
  | { event: "progress"; stage: FlashStage; done: number; total: number | null; bytes_per_sec: number };

/** Fortschritt von `forge` (Rust: `events::ForgeEvent`). */
export type ForgeEvent = { source: "acquire"; event: AcquireEvent } | { source: "flash"; event: FlashEvent };

export interface FlashReport {
  written: number;
  sha256: string;
}

export interface ForgeResult {
  acquired: Acquired;
  report: FlashReport;
}
