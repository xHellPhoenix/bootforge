// Zustand eines Vorgangs für die Fortschrittsansicht:
// - „download“: nur beschaffen + prüfen (Knopf „Nur herunterladen“)
// - „forge“:    beschaffen + prüfen + auf den Stick schreiben (Knopf „Flash!“)
//
// Die Liste der Schritte wird *vorausgeplant* (damit man sieht, was noch kommt) und dann anhand
// der Ereignisse aus Rust aktualisiert.
import { api } from "./api";
import type {
  Acquired,
  AppError,
  Drive,
  FlashMode,
  FlashReport,
  FlashStage,
  ForgeSource,
  LocalImage,
  ResolvedImage,
  Signer,
  Stage,
  WindowsSetup,
} from "./types";

export type StageStatus = "pending" | "active" | "done" | "skipped" | "failed";
export type AnyStage = Stage | FlashStage;

export interface ForgeTarget {
  drive: Drive;
  showExternalHdds: boolean;
  confirmName: string;
  /** Nur Windows-Modus: Antwortdatei-Einstellungen. */
  windowsSetup?: WindowsSetup | null;
}

export interface Job {
  kind: "download" | "forge";
  title: string;
  source: ForgeSource;
  mode: FlashMode;
  target: ForgeTarget | null;
  stages: { id: AnyStage; status: StageStatus }[];
  progress: { stage: AnyStage; done: number; total: number | null; bps: number } | null;
  signer: Signer | null;
  result: Acquired | null;
  flashed: FlashReport | null;
  error: AppError | null;
  running: boolean;
}

/** Schritte beim Schreiben. „split“ (install.wim aufteilen) wird nur bei Bedarf eingefügt. */
function flashStages(mode: FlashMode): AnyStage[] {
  return mode === "windows"
    ? ["check_drive", "verify_image", "prepare", "partition", "format", "copy", "readback"]
    : ["check_drive", "verify_image", "prepare", "write", "readback"];
}

function planned(img: ResolvedImage, forge: boolean): AnyStage[] {
  const s: AnyStage[] = ["checksum"];
  if (img.signature?.signs === "checksum_file") s.push("signature");
  s.push("download", "verify");
  if (img.signature?.signs === "image") s.push("image_signature");
  if (forge) s.push(...flashStages(img.flash_mode));
  return s;
}

/** Job für ein Katalog-Image (herunterladen bzw. herunterladen + schreiben). */
export function createJob(title: string, img: ResolvedImage, target: ForgeTarget | null = null): Job {
  return newJob(title, { type: "catalog", variant_id: img.variant_id }, img.flash_mode, target, planned(img, !!target));
}

/** Job für eine eigene, schon geprüfte Datei (nur schreiben). */
export function createLocalJob(title: string, local: LocalImage, target: ForgeTarget): Job {
  const source: ForgeSource = { type: "local", path: local.acquired.path };
  return newJob(title, source, local.info.flash_mode, target, flashStages(local.info.flash_mode));
}

function newJob(title: string, source: ForgeSource, mode: FlashMode, target: ForgeTarget | null, stages: AnyStage[]): Job {
  // `$state` darf nur bei einer Variablen-Deklaration stehen, nicht direkt hinter `return`.
  const job: Job = $state({
    kind: target ? "forge" : "download",
    title,
    source,
    mode,
    target,
    stages: stages.map((id) => ({ id, status: "pending" as StageStatus })),
    progress: null,
    signer: null,
    result: null,
    flashed: null,
    error: null,
    running: false,
  });
  return job;
}

function enter(job: Job, stage: AnyStage) {
  // Alles vor dem neuen Schritt ist erledigt bzw. wurde übersprungen (z. B. Download bei Cache-Treffer).
  const idx = job.stages.findIndex((s) => s.id === stage);
  if (idx < 0) {
    // Ungeplanter Schritt (Cache-Prüfung, install.wim aufteilen) → direkt hinter dem laufenden.
    const active = job.stages.findIndex((s) => s.status === "active");
    if (active >= 0) job.stages[active].status = "done";
    job.stages.splice(active + 1, 0, { id: stage, status: "active" });
  } else {
    job.stages.forEach((s, i) => {
      if (i < idx && s.status === "active") s.status = "done";
      if (i < idx && s.status === "pending") s.status = "skipped";
    });
    job.stages[idx].status = "active";
  }
  job.progress = null;
}

type ProgressLike = { event: "progress"; stage: AnyStage; done: number; total: number | null; bytes_per_sec: number };
type EventLike = { event: "stage"; stage: AnyStage } | ProgressLike | { event: "signed"; signer: Signer };

function handle(job: Job, e: EventLike) {
  if (e.event === "stage") enter(job, e.stage);
  else if (e.event === "progress") job.progress = { stage: e.stage, done: e.done, total: e.total, bps: e.bytes_per_sec };
  else if (e.event === "signed") job.signer = e.signer;
}

export async function run(job: Job) {
  job.running = true;
  job.error = null;
  job.result = null;
  job.flashed = null;
  for (const s of job.stages) s.status = "pending";
  try {
    if (job.kind === "forge" && job.target) {
      const t = job.target;
      const r = await api.forge(
        job.source,
        t.drive,
        t.showExternalHdds,
        t.confirmName,
        t.windowsSetup ?? null,
        (e) => handle(job, e.event),
      );
      job.result = r.acquired;
      job.flashed = r.report;
    } else if (job.source.type === "catalog") {
      job.result = await api.prepareImage(job.source.variant_id, (e) => handle(job, e));
    }
    for (const s of job.stages) {
      if (s.status === "active") s.status = "done";
      // Aus dem Cache: der Download-Schritt fand nie statt.
      if (s.status === "pending") s.status = "skipped";
    }
  } catch (e) {
    job.error = (e as AppError) ?? { code: "unknown" };
    // Der laufende Schritt ist nicht fertig geworden: bei Abbruch wieder „ausstehend“, sonst „fehlgeschlagen“.
    const cancelled = job.error.code === "cancelled";
    for (const s of job.stages) if (s.status === "active") s.status = cancelled ? "pending" : "failed";
  } finally {
    job.running = false;
    job.progress = null;
  }
}

export function cancel() {
  return api.cancelPrepare();
}
