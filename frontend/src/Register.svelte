<script>
  import { createEventDispatcher } from 'svelte'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import Icon from './Icon.svelte'
  export let docs = []
  const dispatch = createEventDispatcher()

  const TYPES = ['QM', 'QP', 'WI', 'FM']
  const TYPE_LABEL = { QM: 'คู่มือคุณภาพ', QP: 'ระเบียบปฏิบัติ', WI: 'วิธีปฏิบัติงาน', FM: 'แบบฟอร์ม' }

  let q = '', activeType = 'all', fd = '', groupBy = 'type', sortKey = 'code', sortAsc = true

  function sortBy(k) { if (sortKey === k) sortAsc = !sortAsc; else { sortKey = k; sortAsc = true } }
  function clearFilters() { q = ''; activeType = 'all'; fd = '' }
  const sortRows = (rows) => rows.slice().sort((a, b) => {
    const x = (a[sortKey] || '') + '', y = (b[sortKey] || '') + ''
    return (x < y ? -1 : x > y ? 1 : 0) * (sortAsc ? 1 : -1)
  })

  // จำนวนตามประเภท (จากทั้งหมด)
  $: typeCounts = TYPES.reduce((m, t) => { m[t] = docs.filter((d) => d.type === t).length; return m }, {})
  // จำนวนตามแผนก (ตามประเภทที่เลือกอยู่)
  $: deptBase = docs.filter((d) => activeType === 'all' || d.type === activeType)
  $: deptCounts = DEPTS.reduce((m, dp) => { m[dp] = deptBase.filter((d) => d.dept === dp).length; return m }, {})

  $: filtered = docs.filter((d) =>
    (!q || d.code.toLowerCase().includes(q.toLowerCase()) || d.name.toLowerCase().includes(q.toLowerCase())) &&
    (activeType === 'all' || d.type === activeType) &&
    (!fd || d.dept === fd))

  // จัดกลุ่มตามแกนที่เลือก
  $: groups = groupBy === 'type'
    ? TYPES.filter((t) => activeType === 'all' || activeType === t)
        .map((t) => ({ key: t, kind: 'type', label: TYPE_LABEL[t], items: sortRows(filtered.filter((d) => d.type === t)) }))
        .filter((g) => g.items.length)
    : DEPTS
        .map((dp) => ({ key: dp, kind: 'dept', label: DEPTNAME[dp], items: sortRows(filtered.filter((d) => d.dept === dp)) }))
        .filter((g) => g.items.length)

  const hasFilter = () => q || activeType !== 'all' || fd
</script>

<!-- แท็บประเภทเอกสาร -->
<div class="type-tabs">
  <button class="type-tab {activeType === 'all' ? 'active' : ''}" on:click={() => (activeType = 'all')}>
    <Icon name="files" size={15} /> ทั้งหมด <span class="tt-n">{docs.length}</span>
  </button>
  {#each TYPES as t}
    <button class="type-tab t-{t} {activeType === t ? 'active' : ''}" on:click={() => (activeType = t)}>
      <span class="tag {t}">{t}</span> {TYPE_LABEL[t]} <span class="tt-n">{typeCounts[t]}</span>
    </button>
  {/each}
</div>

<!-- แถวเครื่องมือ: ค้นหา + จัดกลุ่มตาม -->
<div class="controls">
  <div class="search-wrap"><span class="search-ic"><Icon name="search" size={16} /></span><input placeholder="ค้นหา รหัส หรือ ชื่อเอกสาร..." bind:value={q} /></div>
  <div class="groupby">
    <span class="gb-label">จัดกลุ่มตาม</span>
    <button class="gb-btn {groupBy === 'type' ? 'active' : ''}" on:click={() => (groupBy = 'type')}>ประเภท</button>
    <button class="gb-btn {groupBy === 'dept' ? 'active' : ''}" on:click={() => (groupBy = 'dept')}>แผนก</button>
  </div>
  {#if hasFilter()}<button class="clearbtn inline-flex items-center gap-1.5" on:click={clearFilters}><Icon name="x" size={14} /> ล้างตัวกรอง</button>{/if}
</div>

<!-- ชิปกรองแผนก -->
<div class="dept-chips">
  <button class="dept-chip {fd === '' ? 'active' : ''}" on:click={() => (fd = '')}>ทุกแผนก <span class="dc-n">{deptBase.length}</span></button>
  {#each DEPTS as dp}
    {#if deptCounts[dp]}
      <button class="dept-chip {fd === dp ? 'active' : ''}" on:click={() => (fd = fd === dp ? '' : dp)}>{DEPTNAME[dp]} <span class="dc-n">{deptCounts[dp]}</span></button>
    {/if}
  {/each}
</div>

<!-- กลุ่มเอกสาร -->
{#each groups as g (g.key)}
  <div class="doc-group">
    <div class="dg-head {g.kind === 'type' ? 't-' + g.key : 'dept'}">
      {#if g.kind === 'type'}<span class="tag {g.key}">{g.key}</span>{:else}<Icon name="folder" size={16} />{/if}
      <span class="dg-title">{g.label}</span>
      <span class="dg-count">{g.items.length} เอกสาร</span>
    </div>
    <div class="dg-table">
      <table>
        <thead><tr>
          <th on:click={() => sortBy('code')}>รหัส</th>
          <th on:click={() => sortBy('name')}>ชื่อเอกสาร</th>
          {#if groupBy !== 'type'}<th on:click={() => sortBy('type')}>ประเภท</th>{/if}
          {#if groupBy !== 'dept'}<th on:click={() => sortBy('deptName')}>แผนก</th>{/if}
          <th on:click={() => sortBy('rev')}>Rev.</th>
          <th on:click={() => sortBy('eff')}>บังคับใช้</th>
          <th>ไฟล์</th>
        </tr></thead>
        <tbody>
          {#each g.items as d (d.code)}
            <tr class={d.unreg ? 'unreg' : ''}>
              <td class="code">{d.code}{#if d.unreg} <span class="badge b-mut text-[9px]">นอกทะเบียน</span>{/if}</td>
              <td>{d.name}{#if d.dup > 1}<br /><span class="dupwarn inline-flex items-center gap-1"><Icon name="alert-triangle" size={12} /> มีไฟล์เก่าซ้ำ {d.dup} ไฟล์ในแฟ้มต้นฉบับ</span>{/if}</td>
              {#if groupBy !== 'type'}<td><span class="tag {d.type}" title="กรองตามประเภท {d.type}" on:click={() => (activeType = d.type)}>{d.type}</span></td>{/if}
              {#if groupBy !== 'dept'}<td class="dept-cell" title="กรองตามแผนก" on:click={() => (fd = d.dept)}>{d.deptName}</td>{/if}
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

<div class="note">แสดง {filtered.length} จาก {docs.length} เอกสาร · {groups.length} {groupBy === 'type' ? 'ประเภท' : 'แผนก'}</div>
