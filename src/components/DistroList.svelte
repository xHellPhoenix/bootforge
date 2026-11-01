<script lang="ts">
  import { lt, t } from "../lib/i18n.svelte";
  import type { Catalog, Distro } from "../lib/types";

  let {
    catalog,
    selected,
    onpick,
  }: { catalog: Catalog; selected: Distro | null; onpick: (d: Distro) => void } = $props();

  let category = $state<string | null>(null);
  let query = $state("");

  // Eine Distro wird angezeigt, wenn mindestens eine Variante zur Kategorie passt
  // und Name/Hersteller die Suche enthält.
  const visible = $derived(
    catalog.distros.filter((d) => {
      const inCategory = !category || d.variants.some((v) => v.categories.includes(category!));
      const q = query.trim().toLowerCase();
      const matches = !q || d.name.toLowerCase().includes(q) || (d.vendor ?? "").toLowerCase().includes(q);
      return inCategory && matches;
    }),
  );
</script>

<section class="list">
  <div class="filters">
    <input type="search" placeholder={t("distro.search")} bind:value={query} />
    <div class="cats" role="group">
      <button class:on={category === null} onclick={() => (category = null)}>{t("distro.all")}</button>
      {#each catalog.categories as c (c.id)}
        <button class:on={category === c.id} onclick={() => (category = c.id)}>{lt(c.name)}</button>
      {/each}
    </div>
  </div>

  <ul>
    {#each visible as d (d.id)}
      <li>
        <button class="distro" class:selected={selected?.id === d.id} onclick={() => onpick(d)}>
          <span class="avatar" aria-hidden="true">{d.name.slice(0, 1)}</span>
          <span class="meta">
            <strong>{d.name}</strong>
            <small>{d.variants.length === 1 ? lt(d.variants[0].name) : t("distro.variant_count", { n: d.variants.length })}</small>
          </span>
        </button>
      </li>
    {:else}
      <li class="empty">{t("distro.empty")}</li>
    {/each}
  </ul>
</section>

<style>
  .list {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--border);
    background: var(--bg-elev);
  }
  .filters {
    padding: 14px;
    display: grid;
    gap: 10px;
    border-bottom: 1px solid var(--border);
  }
  input {
    width: 100%;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .cats {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .cats button {
    padding: 3px 10px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-dim);
    font-size: 12px;
  }
  .cats button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent-hot);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 8px;
    overflow-y: auto;
  }
  .distro {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 9px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
  }
  .distro:hover {
    background: var(--bg-elev-2);
  }
  .distro.selected {
    background: var(--bg-elev-2);
    border-color: var(--accent);
  }
  .avatar {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex: none;
    border-radius: 8px;
    background: var(--bg);
    border: 1px solid var(--border);
    font-weight: 700;
    color: var(--accent-hot);
  }
  .meta {
    display: grid;
  }
  small {
    color: var(--text-dim);
    font-size: 12px;
  }
  .empty {
    padding: 20px;
    color: var(--text-faint);
    text-align: center;
  }
</style>
