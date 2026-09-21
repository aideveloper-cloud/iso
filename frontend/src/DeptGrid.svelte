<script>
  import { createEventDispatcher } from 'svelte'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  export let docs = []
  const dispatch = createEventDispatcher()
</script>

<div class="deptgrid">
  {#each DEPTS as d}
    {@const items = docs.filter((x) => x.dept === d)}
    <div class="deptcard">
      <h3>{DEPTNAME[d]}</h3>
      <div class="cnt">{items.length} เอกสาร · QP {items.filter((i) => i.type === 'QP').length} · FM {items.filter((i) => i.type === 'FM').length} · WI {items.filter((i) => i.type === 'WI').length}</div>
      <ul>
        {#each items as i}
          <li class="doclink" role="button" tabindex="0" on:click={() => dispatch('open', i.code)} on:keydown={(e) => e.key === 'Enter' && dispatch('open', i.code)} title="เปิด / แก้ไข {i.code}">
            <b>{i.code}</b> {i.name}{#if i.versionCount > 1}<span class="verpill">v{i.latestVersion}</span>{/if}<span class="li-open">📄 เปิด</span>
          </li>
        {/each}
      </ul>
    </div>
  {/each}
</div>
