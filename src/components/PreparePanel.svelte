<script lang="ts">
  import { formatBytes, formatDuration } from "../lib/format";
  import { i18n, t } from "../lib/i18n.svelte";
  import { cancel, run, type Job } from "../lib/job.svelte";

  let { job, onclose }: { job: Job; onclose: () => void } = $props();

  const pct = $derived(
    job.progress?.total ? Math.min(100, (job.progress.done / job.progress.total) * 100) : null,
  );
  const eta = $derived(
    job.progress?.total && job.progress.bps > 0
      ? (job.progress.total - job.progress.done) / job.progress.bps
      : null,
  );
  const signerName = $derived(job.signer?.user_id?.replace(/\s*<.*>$/, "") ?? job.signer?.fingerprint ?? "");
</script>

<section class="prepare">
  <h2>{job.title}</h2>

  <ol class="stages">
    {#each job.stages as s (s.id)}
      <li class={s.status}>
        <span class="icon" aria-hidden="true">
          {#if s.status === "done"}✓{:else if s.status === "active"}<span class="spin"></span>{:else if s.status === "skipped"}–{:else if s.status === "failed"}✕{:else}·{/if}
        </span>
        <div class="body">
          <span class="label">
            {t(`stage.${s.id}`)}
            {#if s.status === "skipped"}<em>({t("prepare.skipped")})</em>{/if}
          </span>

          {#if s.status === "done" && (s.id === "signature" || s.id === "image_signature") && job.signer}
            <span class="sub ok">{t("prepare.signed_by", { name: signerName })}</span>
          {/if}

          {#if s.status === "active" && job.progress && job.progress.stage === s.id}
            <div class="bar" role="progressbar" aria-valuenow={pct ?? undefined} aria-valuemin="0" aria-valuemax="100">
              <div class="fill" class:indeterminate={pct === null} style:width={pct === null ? "30%" : `${pct}%`}></div>
            </div>
            <span class="sub">
              {formatBytes(job.progress.done, i18n.locale)}
              {#if job.progress.total} / {formatBytes(job.progress.total, i18n.locale)}{/if}
              {#if job.progress.bps > 0} · {formatBytes(job.progress.bps, i18n.locale)}/s{/if}
              {#if eta !== null} · {t("prepare.eta", { time: formatDuration(eta) })}{/if}
            </span>
          {/if}
        </div>
      </li>
    {/each}
  </ol>

  {#if job.result}
    {@const r = job.result}
    <div class="result {r.assurance}">
      <strong>✓ {t("prepare.done")}</strong>
      <p>{t(`prepare.assurance.${r.assurance}`)}</p>
      {#if r.from_cache}<p class="dim">{t("prepare.from_cache")}</p>{/if}
      <dl>
        <dt>{r.algorithm.toUpperCase()}</dt>
        <dd class="mono">{r.hash}</dd>
        <dt>{t("prepare.saved_at")}</dt>
        <dd class="mono">{r.path}</dd>
      </dl>
    </div>
  {:else if job.error}
    <div class="result error" class:info={job.error.code === "cancelled"}>
      <strong>{job.error.code === "cancelled" ? t("prepare.cancelled") : t(`error.${job.error.code}`)}</strong>
      {#if job.error.detail}
        <details><summary>{t("error.details")}</summary><code>{job.error.detail}</code></details>
      {/if}
    </div>
  {/if}

  <div class="actions">
    {#if job.running}
      <button class="secondary" onclick={cancel}>{t("prepare.cancel")}</button>
    {:else}
      <button class="secondary" onclick={onclose}>{t("prepare.back")}</button>
      {#if job.error && job.error.code !== "checksum_mismatch" && job.error.code !== "signature_invalid"}
        <button class="primary" onclick={() => run(job)}>{t("prepare.resume")}</button>
      {/if}
    {/if}
  </div>
</section>

<style>
  .prepare {
    grid-row: 3;
    padding: 28px 40px;
    overflow-y: auto;
    max-width: 820px;
    width: 100%;
    margin: 0 auto;
  }
  h2 {
    margin: 0 0 22px;
    font-size: 20px;
  }
  .stages {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
  }
  li {
    display: flex;
    gap: 14px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    color: var(--text-faint);
  }
  li.active {
    background: var(--bg-elev);
    color: var(--text);
  }
  li.done {
    color: var(--text-dim);
  }
  .icon {
    display: grid;
    place-items: center;
    flex: none;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 1px solid var(--border);
    font-size: 12px;
  }
  li.done .icon {
    background: var(--ok);
    border-color: var(--ok);
    color: #04140c;
    font-weight: 700;
  }
  li.failed {
    color: var(--danger);
  }
  li.failed .icon {
    border-color: var(--danger);
    color: var(--danger);
  }
  li.active .icon {
    border-color: var(--accent);
  }
  .spin {
    width: 12px;
    height: 12px;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .body {
    display: grid;
    gap: 6px;
    flex: 1;
    min-width: 0;
  }
  em {
    font-style: normal;
    color: var(--text-faint);
  }
  .sub {
    font-size: 12.5px;
    color: var(--text-dim);
  }
  .sub.ok {
    color: var(--ok);
  }
  .bar {
    height: 8px;
    border-radius: 999px;
    background: var(--bg-elev-2);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-hot));
    border-radius: inherit;
    transition: width 0.15s linear;
  }
  .fill.indeterminate {
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  .result {
    margin-top: 22px;
    padding: 16px 18px;
    border-radius: var(--radius);
    border: 1px solid color-mix(in srgb, var(--ok) 40%, transparent);
    background: color-mix(in srgb, var(--ok) 7%, var(--bg));
  }
  .result strong {
    color: var(--ok);
  }
  .result.weak,
  .result.checksum {
    border-color: color-mix(in srgb, var(--warn) 40%, transparent);
  }
  .result.error {
    border-color: color-mix(in srgb, var(--danger) 45%, transparent);
    background: color-mix(in srgb, var(--danger) 7%, var(--bg));
  }
  .result.error strong {
    color: var(--danger);
  }
  .result.error.info {
    border-color: var(--border);
    background: var(--bg-elev);
  }
  .result.error.info strong {
    color: var(--text);
  }
  .result p {
    margin: 6px 0 0;
  }
  .dim {
    color: var(--text-dim);
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 14px;
    margin: 12px 0 0;
    font-size: 12.5px;
  }
  dt {
    color: var(--text-faint);
  }
  dd {
    margin: 0;
    word-break: break-all;
  }
  .mono,
  code {
    font-family: var(--mono);
    font-size: 12px;
    user-select: text;
  }
  code {
    color: var(--text-dim);
    word-break: break-all;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 22px;
  }
  .actions button {
    padding: 8px 20px;
    border-radius: var(--radius-sm);
    font-weight: 600;
  }
  .secondary {
    border: 1px solid var(--border);
    background: var(--bg-elev-2);
  }
  .primary {
    border: none;
    color: #1a0d00;
    background: linear-gradient(180deg, var(--accent-hot), var(--accent));
  }
</style>
