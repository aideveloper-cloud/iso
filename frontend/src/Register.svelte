<script>
  import { createEventDispatcher } from 'svelte'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  export let docs = []
  const dispatch = createEventDispatcher()
  let q = '', ft = '', fd = '', sortKey = 'code', sortAsc = true, view = 'table'
  function sortBy(k) { if (sortKey === k) sortAsc = !sortAsc; else { sortKey = k; sortAsc = true } }
  function clearFilters() { q = ''; ft = ''; fd = '' }
  const filterByType = (t) => (ft = t)
  const filterByDept = (d) => (fd = d)

  $: filtered = docs.filter((d) => (!q || d.code.toLowerCase().includes(q.toLowerCase()) || d.name.toLowerCase().includes(q.toLowerCase())) && (!ft || d.type === ft) && (!fd || d.dept === fd))
  $: rows = filtered
    .slice()
    .sort((a, b) => { const x = (a[sortKey] || '') + '', y = (b[sortKey] || '') + ''; return (x < y ? -1 : x > y ? 1 : 0) * (sortAsc ? 1 : -1) })
  $: deptList = fd ? [fd] : DEPTS
</script>

<div class="controls">
  <input placeholder="🔍 ค้นหา รหัส หรือ ชื่อเอกสาร..." bind:value={q} />
  <select bind:value={ft}><option value="">ทุกประเภท</option><option>QM</option><option>QP</option><option>WI</option><option>FM</option></select>
  <select bind:value={fd}><option value="">ทุกแผนก</option>{#each DEPTS as d}<option value={d}>{DEPTNAME[d]}</option>{/each}</select>
  {#if q || ft || fd}<button class="clearbtn" on:click={clearFilters}>✕ ล้างตัวกรอง</button>{/if}
</div>

<div class="viewtoggle">
  <button class="vt-btn {view === 'table' ? 'active' : ''}" on:click={() => (view = 'table')}>📋 มุมมองตาราง</button>
  <button class="vt-btn {view === 'card' ? 'active' : ''}" on:click={() => (view = 'card')}>🗂️ มุมมองตามแผนก</button>
</div>

{#if view === 'table'}
  <div class="sec" style="padding:0;overflow:auto;max-height:calc(100vh - 260px)">
    <table id="tbl">
      <thead><tr>
        <th on:click={() => sortBy('code')}>รหัส</th><th on:click={() => sortBy('name')}>ชื่อเอกสาร</th>
        <th on:click={() => sortBy('type')}>ประเภท</th><th on:click={() => sortBy('deptName')}>แผนก</th>
        <th on:click={() => sortBy('rev')}>Rev.</th><th on:click={() => sortBy('eff')}>บังคับใช้</th><th>ไฟล์</th>
      </tr></thead>
      <tbody>
        {#each rows as d}
          <tr class={d.unreg ? 'unreg' : ''}>
            <td class="code">{d.code}{#if d.unreg} <span class="badge b-mut" style="font-size:9px">นอกทะเบียน</span>{/if}</td>
            <td>{d.name}{#if d.dup > 1}<br /><span class="dupwarn">⚠ มีไฟล์เก่าซ้ำ {d.dup} ไฟล์ในแฟ้มต้นฉบับ</span>{/if}</td>
            <td><span class="tag {d.type}" title="กรองตามประเภท {d.type}" on:click={() => filterByType(d.type)}>{d.type}</span></td>
            <td class="dept-cell" title="กรองตามแผนก" on:click={() => filterByDept(d.dept)}>{d.deptName}</td><td>{d.rev}</td><td>{d.eff}</td>
            <td><button class="open" on:click={() => dispatch('open', d.code)} title={d.file}>📄 เปิด / แก้ไข</button>{#if d.versionCount > 1}<span class="verpill" title="มี {d.versionCount} เวอร์ชัน">v{d.latestVersion}</span>{/if}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}
  <div class="deptgrid">
    {#each deptList as d}
      {@const items = filtered.filter((x) => x.dept === d)}
      {#if items.length}
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
      {/if}
    {/each}
    {#if !filtered.length}<p class="mut" style="padding:20px 4px">ไม่พบเอกสารที่ตรงกับตัวกรอง</p>{/if}
  </div>
{/if}

<div class="note">แสดง {rows.length} จาก {docs.length} เอกสาร</div>
