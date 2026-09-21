<script>
  import { createEventDispatcher } from 'svelte'
  import { DEPTS, DEPTNAME, PROC_READY, TYPE_ORDER } from './lib/const.js'
  export let docs = []
  const dispatch = createEventDispatcher()
  let active = 'MR', q = '', showAll = false
  $: items = docs.filter((x) => x.dept === active && (!q || x.code.toLowerCase().includes(q.toLowerCase()) || x.name.toLowerCase().includes(q.toLowerCase())))
</script>

<div class="cat-head"><h2 class="cat-title">📋 วิธีปฏิบัติงานแต่ละแผนก</h2><p class="cat-desc">ระเบียบปฏิบัติ (QP) · วิธีปฏิบัติงาน (WI) · แบบฟอร์ม (FM) แยกตามแผนก — กด "เปิด / แก้ไข" ได้เลย</p></div>
<div class="subtabs">
  {#each PROC_READY as d}
    <button class="pd-btn {d === active ? 'active' : ''}" on:click={() => (active = d)}>{DEPTNAME[d]} <span class="pd-n">{docs.filter((x) => x.dept === d).length}</span></button>
  {/each}
  <button class="pd-more" on:click={() => (showAll = !showAll)}>แผนกอื่น ๆ {showAll ? '▲' : '▾'}</button>
  {#if showAll}
    <div class="pd-others">
      {#each DEPTS.filter((d) => !PROC_READY.includes(d)) as d}
        <button class="pd-btn {d === active ? 'active' : ''}" on:click={() => (active = d)}>{DEPTNAME[d]} <span class="pd-n">{docs.filter((x) => x.dept === d).length}</span></button>
      {/each}
    </div>
  {/if}
</div>
<input class="cat-search" placeholder="🔍 ค้นหา รหัส หรือ ชื่อเอกสาร ในแผนกนี้..." bind:value={q} />
<h3 class="proc-title">{DEPTNAME[active]} <span class="mut">· {items.length} เอกสาร</span></h3>
{#each TYPE_ORDER as [t, label]}
  {@const g = items.filter((x) => x.type === t)}
  {#if g.length}
    <div class="proc-group">
      <div class="pg-head"><span class="tag {t}">{t}</span> {label} <span class="mut">({g.length})</span></div>
      {#each g as d}
        <div class="proc-row">
          <div class="pr-info"><b class="code">{d.code}</b> {d.name} <span class="mut">· {d.rev}</span>{#if d.versionCount > 1}<span class="verpill">v{d.latestVersion}</span>{/if}</div>
          <button class="open" on:click={() => dispatch('open', d.code)}>📄 เปิด / แก้ไข</button>
        </div>
      {/each}
    </div>
  {/if}
{/each}
{#if items.length === 0}<p class="mut" style="padding:20px 4px">ไม่พบเอกสารในแผนกนี้</p>{/if}
