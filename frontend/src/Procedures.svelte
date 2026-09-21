<script>
  import { createEventDispatcher } from 'svelte'
  import { DEPTS, DEPTNAME, TYPE_ORDER } from './lib/const.js'
  import Icon from './Icon.svelte'
  export let docs = []
  const dispatch = createEventDispatcher()

  let activeDept = 'all', ptype = '', q = '', sortKey = 'code', sortAsc = true

  function sortBy(k) { if (sortKey === k) sortAsc = !sortAsc; else { sortKey = k; sortAsc = true } }
  const sortRows = (rows) => rows.slice().sort((a, b) => {
    const x = (a[sortKey] || '') + '', y = (b[sortKey] || '') + ''
    return (x < y ? -1 : x > y ? 1 : 0) * (sortAsc ? 1 : -1)
  })
  const clearFilters = () => { activeDept = 'all'; ptype = ''; q = '' }
  const hasFilter = () => activeDept !== 'all' || ptype || q

  // จำนวนตามแผนก (จากทั้งหมด)
  $: deptCounts = DEPTS.reduce((m, dp) => { m[dp] = docs.filter((d) => d.dept === dp).length; return m }, {})
  // ฐานตามแผนกที่เลือก
  $: base = docs.filter((d) => activeDept === 'all' || d.dept === activeDept)
  // จำนวนตามประเภท (ในแผนกที่เลือก)
  $: typeCounts = TYPE_ORDER.reduce((m, [t]) => { m[t] = base.filter((d) => d.type === t).length; return m }, {})

  $: filtered = base.filter((d) =>
    (ptype === '' || d.type === ptype) &&
    (!q || d.code.toLowerCase().includes(q.toLowerCase()) || d.name.toLowerCase().includes(q.toLowerCase())))

  // จัดกลุ่มตามประเภทเสมอ (ตามลำดับ ISO)
  $: groups = TYPE_ORDER
    .map(([t, label]) => ({ key: t, label, items: sortRows(filtered.filter((d) => d.type === t)) }))
    .filter((g) => g.items.length)
</script>

<div class="cat-head"><h2 class="cat-title inline-flex items-center gap-2"><Icon name="clipboard-list" size={20} /> วิธีปฏิบัติงานแต่ละแผนก</h2><p class="cat-desc">ระเบียบปฏิบัติ (QP) · วิธีปฏิบัติงาน (WI) · แบบฟอร์ม (FM) แยกตามแผนก — เลือกแผนกแล้วดูเอกสารแยกตามประเภท</p></div>

<!-- แท็บแผนก -->
<div class="type-tabs">
  <button class="type-tab {activeDept === 'all' ? 'active' : ''}" on:click={() => (activeDept = 'all')}>
    <Icon name="files" size={15} /> ทุกแผนก <span class="tt-n">{docs.length}</span>
  </button>
  {#each DEPTS as dp}
    {#if deptCounts[dp]}
      <button class="type-tab {activeDept === dp ? 'active' : ''}" on:click={() => (activeDept = dp)}>
        <Icon name="folder" size={15} /> {DEPTNAME[dp]} <span class="tt-n">{deptCounts[dp]}</span>
      </button>
    {/if}
  {/each}
</div>

<!-- ค้นหา -->
<div class="controls">
  <div class="search-wrap"><span class="search-ic"><Icon name="search" size={16} /></span><input placeholder="ค้นหา รหัส หรือ ชื่อเอกสาร..." bind:value={q} /></div>
  {#if hasFilter()}<button class="clearbtn inline-flex items-center gap-1.5" on:click={clearFilters}><Icon name="x" size={14} /> ล้างตัวกรอง</button>{/if}
</div>

<!-- ชิปกรองประเภท -->
<div class="dept-chips">
  <button class="dept-chip {ptype === '' ? 'active' : ''}" on:click={() => (ptype = '')}>ทุกประเภท <span class="dc-n">{base.length}</span></button>
  {#each TYPE_ORDER as [t, label]}
    {#if typeCounts[t]}
      <button class="dept-chip {ptype === t ? 'active' : ''}" on:click={() => (ptype = ptype === t ? '' : t)}><span class="tag {t}">{t}</span> {label.replace(/\s*\(.*\)/, '')} <span class="dc-n">{typeCounts[t]}</span></button>
    {/if}
  {/each}
</div>

<!-- กลุ่มเอกสารตามประเภท -->
{#each groups as g (g.key)}
  <div class="doc-group">
    <div class="dg-head t-{g.key}">
      <span class="tag {g.key}">{g.key}</span>
      <span class="dg-title">{g.label}</span>
      <span class="dg-count">{g.items.length} เอกสาร</span>
    </div>
    <div class="dg-table">
      <table>
        <thead><tr>
          <th on:click={() => sortBy('code')}>รหัส</th>
          <th on:click={() => sortBy('name')}>ชื่อเอกสาร</th>
          {#if activeDept === 'all'}<th on:click={() => sortBy('deptName')}>แผนก</th>{/if}
          <th on:click={() => sortBy('rev')}>Rev.</th>
          <th on:click={() => sortBy('eff')}>บังคับใช้</th>
          <th>ไฟล์</th>
        </tr></thead>
        <tbody>
          {#each g.items as d (d.code)}
            <tr class={d.unreg ? 'unreg' : ''}>
              <td class="code">{d.code}{#if d.unreg} <span class="badge b-mut text-[9px]">นอกทะเบียน</span>{/if}</td>
              <td>{d.name}{#if d.dup > 1}<br /><span class="dupwarn inline-flex items-center gap-1"><Icon name="alert-triangle" size={12} /> มีไฟล์เก่าซ้ำ {d.dup} ไฟล์ในแฟ้มต้นฉบับ</span>{/if}</td>
              {#if activeDept === 'all'}<td class="dept-cell" title="เลือกแผนกนี้" on:click={() => (activeDept = d.dept)}>{d.deptName}</td>{/if}
              <td>{d.rev}</td><td>{d.eff}</td>
              <td><button class="open inline-flex items-center gap-1.5" on:click={() => dispatch('open', d.code)} title={d.file}><Icon name="file-text" size={14} /> เปิด / แก้ไข</button>{#if d.versionCount > 1}<span class="verpill" title="มี {d.versionCount} เวอร์ชัน">v{d.latestVersion}</span>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
{/each}

{#if !filtered.length}
  <div class="empty-state">
    <Icon name="search" size={38} stroke={1.5} />
    <p>ไม่พบเอกสารที่ตรงกับตัวกรอง</p>
    {#if hasFilter()}<button class="mbtn gh" on:click={clearFilters}>ล้างตัวกรองทั้งหมด</button>{/if}
  </div>
{/if}

<div class="note">แสดง {filtered.length} เอกสาร{#if activeDept !== 'all'} · {DEPTNAME[activeDept]}{/if} · {groups.length} ประเภท</div>
