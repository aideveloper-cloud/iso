<script>
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME, matchesQuery } from './lib/const.js'
  import Icon from './Icon.svelte'
  export let docs = []
  const dispatch = createEventDispatcher()

  const TYPES = ['QM', 'QP', 'WI', 'FM']
  const TYPE_LABEL = { QM: 'คู่มือคุณภาพ', QP: 'ระเบียบปฏิบัติ', WI: 'วิธีปฏิบัติงาน', FM: 'แบบฟอร์ม' }

  let q = '', ft = '', fd = '', onlyIssues = false
  let actions = []

  onMount(async () => { try { actions = await api.actions() } catch (e) {} })

  // นับ CAR/DAR/PAR ที่ยังไม่ปิด ต่อรหัสเอกสาร
  $: openByDoc = actions.reduce((m, a) => {
    if (a.status !== 'closed' && a.docCode) m[a.docCode] = (m[a.docCode] || 0) + 1
    return m
  }, {})
  function issuesOf(d) {
    const list = []
    if (d.unreg) list.push({ t: 'นอกทะเบียน', c: 'warn' })
    if (d.missing) list.push({ t: 'ไฟล์หาย', c: 'red' })
    if (d.dup > 1) list.push({ t: `ไฟล์ซ้ำ ${d.dup}`, c: 'warn' })
    if (openByDoc[d.code]) list.push({ t: `CAR/DAR/PAR ค้าง ${openByDoc[d.code]}`, c: 'red' })
    return list
  }

  $: rows = docs.filter((d) =>
    matchesQuery(d, q) &&
    (!ft || d.type === ft) && (!fd || d.dept === fd) &&
    (!onlyIssues || issuesOf(d).length > 0))

  $: summary = {
    total: docs.length,
    revised: docs.filter((d) => (d.versionCount || 1) > 1).length,
    unreg: docs.filter((d) => d.unreg).length,
    missing: docs.filter((d) => d.missing).length,
    openActions: actions.filter((a) => a.status !== 'closed').length,
    issues: docs.filter((d) => issuesOf(d).length > 0).length,
  }
  function clearFilters() { q = ''; ft = ''; fd = ''; onlyIssues = false }
</script>

<div class="cat-head no-print">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="list-checks" size={20} /> เตรียม Audit</h2>
  <p class="cat-desc">ดัชนีเอกสารควบคุมพร้อมสถานะความพร้อมตรวจประเมิน — ค้นหา เรียกดู และพิมพ์รายการสำหรับ Auditor ได้ในคลิกเดียว</p>
</div>

<!-- สรุปสำหรับ audit -->
<div class="audit-summary">
  <div class="as-card"><div class="as-n">{summary.total}</div><div class="as-l">เอกสารควบคุมทั้งหมด</div></div>
  <div class="as-card"><div class="as-n">{summary.revised}</div><div class="as-l">มีการแก้ไข (>1 เวอร์ชัน)</div></div>
  <div class="as-card warn"><div class="as-n">{summary.unreg}</div><div class="as-l">นอกทะเบียน</div></div>
  <div class="as-card red"><div class="as-n">{summary.missing}</div><div class="as-l">ไฟล์หาย</div></div>
  <div class="as-card red"><div class="as-n">{summary.openActions}</div><div class="as-l">CAR/DAR/PAR ค้าง</div></div>
  <div class="as-card red"><div class="as-n">{summary.issues}</div><div class="as-l">เอกสารมีประเด็น</div></div>
</div>

<div class="controls no-print">
  <div class="search-wrap"><span class="search-ic"><Icon name="search" size={16} /></span><input placeholder="ค้นหา รหัส หรือ ชื่อเอกสาร..." bind:value={q} /></div>
  <select bind:value={ft}><option value="">ทุกประเภท</option>{#each TYPES as t}<option value={t}>{TYPE_LABEL[t]} ({t})</option>{/each}</select>
  <select bind:value={fd}><option value="">ทุกแผนก</option>{#each DEPTS as d}<option value={d}>{DEPTNAME[d]}</option>{/each}</select>
  <label class="issue-toggle"><input type="checkbox" bind:checked={onlyIssues} /> เฉพาะที่มีประเด็น</label>
  {#if q || ft || fd || onlyIssues}<button class="clearbtn inline-flex items-center gap-1.5" on:click={clearFilters}><Icon name="x" size={14} /> ล้าง</button>{/if}
  <button class="mbtn inline-flex items-center gap-1.5" on:click={() => window.print()}><Icon name="printer" size={14} /> พิมพ์รายการ</button>
</div>

<div class="print-title">ดัชนีเอกสารควบคุม ISO 9001:2015 — เตรียมตรวจประเมิน (แสดง {rows.length} รายการ)</div>

<div class="sec !p-0 overflow-auto">
  <table id="tbl">
    <thead><tr>
      <th>รหัส</th><th>ชื่อเอกสาร</th><th>ประเภท</th><th>แผนก</th><th>Rev.</th><th>บังคับใช้</th><th>เวอร์ชัน</th><th>สถานะ</th>
    </tr></thead>
    <tbody>
      {#each rows as d (d.code)}
        {@const iss = issuesOf(d)}
        <tr class="clickrow" on:click={() => dispatch('open', d.code)} title="คลิกเพื่อเปิดดู">
          <td class="code">{d.code}</td>
          <td>{d.name}</td>
          <td><span class="tag {d.type}">{d.type}</span></td>
          <td>{d.deptName}</td>
          <td>{d.rev}</td>
          <td>{d.eff}</td>
          <td>{#if (d.versionCount || 1) > 1}<span class="verpill">v{d.latestVersion}</span> <span class="mut">({d.versionCount} เวอร์ชัน)</span>{:else}<span class="mut">ต้นฉบับ</span>{/if}</td>
          <td>
            {#if iss.length}
              {#each iss as x}<span class="audit-flag f-{x.c}">{x.t}</span>{/each}
            {:else}
              <span class="audit-flag f-ok inline-flex items-center gap-1"><Icon name="circle-check" size={12} /> พร้อม</span>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if !rows.length}<div class="empty-state"><Icon name="search" size={34} stroke={1.5} /><p>ไม่พบเอกสารที่ตรงกับตัวกรอง</p></div>{/if}
</div>
