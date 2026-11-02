// Zustand eines Beschaffungs-Vorgangs (Download + Prüfung) für die Fortschrittsansicht.
//
// Die Liste der Schritte wird aus dem ermittelten Image *vorausgeplant* (damit man sieht, was
// noch kommt) und dann anhand der Ereignisse aus Rust aktualisiert.
import { api } from "./api";
import type { Acquired, AppError, ResolvedImage, Signer, Stage } from "./types";

export type StageStatus = "pending" | "active" | "done" | "skipped" | "failed";

export interface Job {
  title: string;
  variantId: string;
  stages: { id: Stage; status: StageStatus }[];
  progress: { stage: Stage; done: number; total: number | null; bps: number } | null;
  signer: Signer | null;
  result: Acquired | null;
  error: AppError | null;
  running: boolean;
}

function planned(img: ResolvedImage): Stage[] {
  const s: Stage[] = ["checksum"];
  if (img.signature?.signs === "checksum_file") s.push("signature");
  s.push("download", "verify");
  if (img.signature?.signs === "image") s.push("image_signature");
  return s;
}

export function createJob(title: string, img: ResolvedImage): Job {
  // `$state` darf nur bei einer Variablen-Deklaration stehen, nicht direkt hinter `return`.
  const job: Job = $state({
    title,
    variantId: img.variant_id,
    stages: planned(img).map((id) => ({ id, status: "pending" as StageStatus })),
    progress: null,
    signer: null,
    result: null,
    error: null,
    running: false,
  });
  return job;
}

function enter(job: Job, stage: Stage) {
  // Bisher aktive Schritte sind erledigt.
  for (const s of job.stages) if (s.status === "active") s.status = "done";
  let entry = job.stages.find((s) => s.id === stage);
  if (!entry) {
    // Ungeplanter Schritt (Cache-Prüfung) → vor dem Download einfügen.
    entry = { id: stage, status: "pending" };
    const at = job.stages.findIndex((s) => s.id === "download");
    job.stages.splice(at < 0 ? job.stages.length : at, 0, entry);
  }
  entry.status = "active";
  job.progress = null;
}

export async function run(job: Job) {
  job.running = true;
  job.error = null;
  job.result = null;
  for (const s of job.stages) s.status = "pending";
  try {
    const result = await api.prepareImage(job.variantId, (e) => {
      if (e.event === "stage") enter(job, e.stage);
      else if (e.event === "progress")
        job.progress = { stage: e.stage, done: e.done, total: e.total, bps: e.bytes_per_sec };
      else if (e.event === "signed") job.signer = e.signer;
    });
    for (const s of job.stages) {
      if (s.status === "active") s.status = "done";
      // Aus dem Cache: der Download-Schritt fand nie statt.
      if (s.status === "pending") s.status = "skipped";
    }
    job.result = result;
  } catch (e) {
    job.error = (e as AppError) ?? { code: "unknown" };
    // Der laufende Schritt ist nicht fertig geworden: bei Abbruch wieder „ausstehend“, sonst „fehlgeschlagen“.
    for (const s of job.stages)
      if (s.status === "active") s.status = job.error.code === "cancelled" ? "pending" : "failed";
  } finally {
    job.running = false;
    job.progress = null;
  }
}

export function cancel() {
  return api.cancelPrepare();
}
