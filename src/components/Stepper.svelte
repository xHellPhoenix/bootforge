<script lang="ts">
  import { t } from "../lib/i18n.svelte";

  let { current }: { current: number } = $props();
  const steps = ["step.distro", "step.usb", "step.forge"];
</script>

<ol class="stepper">
  {#each steps as key, i (key)}
    {@const n = i + 1}
    <li class:done={n < current} class:active={n === current}>
      <span class="num">{n < current ? "✓" : n}</span>
      <span>{t(key)}</span>
    </li>
  {/each}
</ol>

<style>
  .stepper {
    display: flex;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px 6px 6px;
    border-radius: 999px;
    color: var(--text-faint);
    font-size: 13px;
  }
  li + li::before {
    content: "";
    width: 18px;
    height: 1px;
    background: var(--border);
    margin-right: 6px;
  }
  .num {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 1px solid var(--border);
    font-size: 12px;
  }
  .active {
    color: var(--text);
  }
  .active .num {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent-hot);
  }
  .done {
    color: var(--text-dim);
  }
  .done .num {
    background: var(--accent);
    border-color: var(--accent);
    color: #1a0d00;
  }
</style>
