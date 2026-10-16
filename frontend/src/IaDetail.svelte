<script>
  // การตรวจติดตามภายในหนึ่งครั้ง: แผน · checklist + ผลการตรวจ · ข้อบกพร่อง → CAR · ลงนามรายงาน · ปิด
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTNAME } from './lib/const.js'
  import { fmtDate, fmtWhen } from './lib/changeNote.js'
  import DecisionDialog from './DecisionDialog.svelte'
  import Icon from './Icon.svelte'

  export let id
  export let me = null
  export let docs = []
  const dispatch = createEventDispatcher()

  const STATUS = { planned: 'วางแผน', in_progress: 'กำลังตรวจ', reported: 'ลงนามรายงานแล้ว', closed: 'ปิดแล้ว', cancelled: 'ยกเลิก' }
  const RESULT = { pending: 'ยังไม่ตรวจ', conform: 'สอดคล้อง', ofi: 'ข้อสังเกต (OFI)', minor: 'NC เล็กน้อย', major: 'NC สำคัญ', na: 'ไม่เกี่ยวข้อง' }
  const CAR = { open: 'เปิด', in_progress: 'กำลังดำเนินการ', verify: 'รอทวนสอบ', closed: 'ปิดแล้ว', cancelled: 'ยกเลิก' }
  let a = null, err = '', msg = '', deciding = null, edit = null, saving = false, filter = 'all'
  const docName = (c) => docs.find((d) => d.code === c)?.name || ''

  async function load() {
    err = ''
    try { const r = await api.iaGet(id); if (r.error) throw new Error(r.error); a = r } catch (e) { err = 'โหลดไม่สำเร็จ: ' + e.message }
  }
  onMount(load)
  function apply(r, okMsg) {
    if (!r.ok) { err = r.body.error || 'บันทึกไม่สำเร็จ'; if (r.status === 409) load(); return false }
    a = r.body; err = ''; msg = okMsg || ''
    return true
  }

  $: can = a?.can || {}
  $: shown = a ? a.items.filter((i) => filter === 'all' || (filter === 'nc' ? ['minor', 'major'].includes(i.result) : filter === 'pending' ? i.result === 'pending' : i.result === filter)) : []

  function openItem(it) { edit = it ? { ...it } : { id: null, clause: '', question: '', result: 'pending', evidence: '', finding: '', dept: '' }; err = '' }
  async function saveItem(remove = false) {
    saving = true
    const r = await api.iaItem(a.id, remove ? { id: edit.id, remove: true } : { id: edit.id || undefined, clause: edit.clause, question: edit.question, result: edit.result, evidence: edit.evidence, finding: edit.finding, dept: edit.dept })
    saving = false
    if (apply(r, remove ? 'ลบข้อแล้ว' : 'บันทึกผลแล้ว')) edit = null
  }
  async function raiseCar(it) { apply(await api.iaRaiseCar(a.id, it.id), `เปิด CAR จากข้อ ${it.clause} แล้ว — ติดตามต่อในเมนู CAR / DAR / PAR`) }

  const DECIDE = {
    start: { title: 'เริ่มตรวจ', label: 'เริ่มตรวจ', comment: 'หมายเหตุ', noPin: true },
    report: { title: 'ลงนามรายงานการตรวจ (หัวหน้าผู้ตรวจ)', label: 'ลงนามรายงาน', comment: 'สรุปผลการตรวจ (จุดแข็ง ข้อบกพร่อง ข้อสังเกต)', required: true },
    close: { title: 'ปิดการตรวจ', label: 'ปิดการตรวจ', comment: 'หมายเหตุ' },
    cancel: { title: 'ยกเลิกการตรวจ', label: 'ยกเลิก', comment: 'เหตุผล', required: true, danger: true, noPin: true },
  }
  $: dd = deciding ? DECIDE[deciding] : null
  async function onDecide(e) {
    const { comment, pin, fail } = e.detail
    const r = await api.iaAct(a.id, a.version, deciding, comment, pin)
    if (!r.ok) { fail(r.body.error || 'บันทึกไม่สำเร็จ'); return }
    const was = deciding; deciding = null
    apply(r, { start: 'เริ่มตรวจแล้ว — สร้าง checklist จากแผนผังข้อกำหนด', report: 'ลงนามรายงานแล้ว', close: 'ปิดการตรวจแล้ว', cancel: 'ยกเลิกแล้ว' }[was])
  }
</script>

<div class="flex flex-wrap items-center gap-2 mb-3">
  <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => dispatch('back')}><Icon name="chevrons-left" size={14} /> แผนการตรวจ</button>
  {#if a}<b class="text-brand text-[16px]">{a.refNo}</b> <span class="status-badge ia-{a.status}">{STATUS[a.status]}</span> <span class="mut">{a.title}</span>{/if}
</div>
{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>โหลดใหม่</button></div>{/if}
{#if msg}<div class="save-ok">{msg}</div>{/if}

{#if a}
  <div class="mr-bar">
    {#if can.start}<button class="mbtn accent" on:click={() => (deciding = 'start')}>เริ่มตรวจ</button>{/if}
    {#if a.status === 'in_progress' && (a.leadCode === me?.empCode || me?.role === 'admin')}
      <button class="mbtn accent inline-flex items-center gap-1.5" disabled={!can.report} title={can.report ? '' : `ค้างบันทึกผล ${can.pending} ข้อ · NC ที่ยังไม่เปิด CAR ${can.ncWithoutCar}`} on:click={() => (deciding = 'report')}><Icon name="pen-line" size={14} /> ลงนามรายงาน</button>
      {#if !can.report}<span class="mnote">ก่อนลงนาม: บันทึกผลให้ครบ (ค้าง {can.pending}) และเปิด CAR ให้ทุก NC (ยังไม่เปิด {can.ncWithoutCar})</span>{/if}
    {/if}
    {#if can.close}<button class="mbtn accent" on:click={() => (deciding = 'close')}>ปิดการตรวจ</button>{/if}
    {#if a.status === 'reported' && !can.close}<span class="mnote">ปิดได้โดยผู้อนุมัติที่ไม่อยู่ในทีมผู้ตรวจ เมื่อ CAR ปิดครบ (เปิดอยู่ {can.openCars})</span>{/if}
    {#if can.cancel}<button class="mbtn gh" on:click={() => (deciding = 'cancel')}>ยกเลิกการตรวจ</button>{/if}
  </div>

  <div class="capa-grid mb-3">
    <div class="sec">
      <h2 class="!mb-2">แผนการตรวจ</h2>
      <div class="det-info">
        <div><b>ปี:</b> {a.year}</div><div><b>วันที่:</b> {a.actualDate ? fmtDate(a.actualDate) : a.plannedDate ? 'แผน ' + fmtDate(a.plannedDate) : '—'}</div>
        <div class="col-span-2"><b>ข้อกำหนด:</b> {a.clauses.join(', ')}</div>
        <div class="col-span-2"><b>แผนก:</b> {a.depts.map((d) => DEPTNAME[d] || d).join(', ') || '—'}</div>
        <div class="col-span-2"><b>เกณฑ์:</b> {a.criteria || '—'}</div>
        <div><b>ผู้รับการตรวจ:</b> {a.auditeeName || '—'}</div><div><b>วางแผนโดย:</b> {a.createdName}</div>
      </div>
    </div>
    <div class="sec">
      <h2 class="!mb-2">ทีมผู้ตรวจ</h2>
      {#each a.team as m, i}<div class="text-[13px] py-[3px]">{i === 0 ? '★ หัวหน้า: ' : '• '}<b>{m.name}</b> <span class="mut">{DEPTNAME[m.dept] || m.dept}</span></div>{/each}
      <div class="det-info mt-3">
        <div><b>สอดคล้อง:</b> {a.summaryCounts.conform}</div><div><b>ข้อสังเกต:</b> {a.summaryCounts.ofi}</div>
        <div><b>NC เล็กน้อย:</b> {a.summaryCounts.minor}</div><div><b>NC สำคัญ:</b> {a.summaryCounts.major}</div>
        <div><b>ยังไม่ตรวจ:</b> {a.summaryCounts.pending}</div><div><b>ไม่เกี่ยวข้อง:</b> {a.summaryCounts.na}</div>
      </div>
    </div>
  </div>

  {#if a.reportAt}
    <div class="sec mb-3"><h2 class="!mb-1">รายงานการตรวจ</h2>
      <div class="whitespace-pre-line text-[13px]">{a.summary}</div>
      <p class="mut text-[11.5px] mb-0">ลงนามโดย {a.reportBy} · {fmtWhen(a.reportAt)} · SHA-256 {a.reportSha.slice(0, 16)}{#if a.closedAt} · ปิดโดย {a.closedBy} {fmtWhen(a.closedAt)}{/if}</p>
    </div>
  {/if}
  {#if a.cancelReason}<div class="merr">ยกเลิก: {a.cancelReason}</div>{/if}

  {#if a.items.length}
    <div class="flex flex-wrap items-center gap-2 mb-2">
      <h3 class="rd-sub !m-0">Checklist ({a.items.length})</h3>
      <span class="flex-1"></span>
      <div class="subtabs !mb-0">
        {#each [['all', 'ทั้งหมด'], ['pending', 'ยังไม่ตรวจ'], ['nc', 'NC'], ['ofi', 'OFI'], ['conform', 'สอดคล้อง']] as [k, l]}<button class="st-btn {filter === k ? 'active' : ''}" on:click={() => (filter = k)}>{l}</button>{/each}
      </div>
      {#if can.conduct}<button class="mbtn gh" on:click={() => openItem(null)}>+ เพิ่มข้อ</button>{/if}
    </div>
    <div class="sec !p-0 overflow-auto">
      <table class="chg-table">
        <thead><tr><th>ข้อ</th><th>สิ่งที่ตรวจ</th><th>ผล</th><th>หลักฐาน / ข้อพบ</th><th>CAR</th><th></th></tr></thead>
        <tbody>
          {#each shown as it (it.id)}
            <tr>
              <td class="nowrap"><b>{it.clause || '—'}</b></td>
              <td>{it.question}{#if it.docRefs.length}<div class="rd-ev !mt-1">{#each it.docRefs as d}<span class="rd-chip doc" title={docName(d)}>{d}</span>{/each}</div>{/if}</td>
              <td class="nowrap"><span class="ia-res r-{it.result}">{RESULT[it.result]}</span></td>
              <td class="text-[12px]">{#if it.evidence}<div><b class="text-mut">หลักฐาน:</b> {it.evidence}</div>{/if}{#if it.finding}<div><b class="text-mut">ข้อพบ:</b> {it.finding}</div>{/if}{#if it.updatedBy}<div class="mut text-[10.5px]">{it.updatedBy} · {fmtWhen(it.updatedAt)}</div>{/if}</td>
              <td class="nowrap">{#if it.carRef}<b>{it.carRef}</b><br /><span class="mut">{CAR[it.carStatus] || ''}</span>{:else if can.conduct && ['minor', 'major'].includes(it.result)}<button class="mbtn danger" on:click={() => raiseCar(it)}>เปิด CAR</button>{:else}—{/if}</td>
              <td>{#if can.conduct}<button class="mbtn gh" on:click={() => openItem(it)}>บันทึก</button>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if a.status === 'planned'}
    <p class="mnote">เมื่อกด “เริ่มตรวจ” ระบบจะสร้าง checklist จากข้อกำหนดที่เลือก พร้อมเอกสารอ้างอิงของแผนกที่ถูกตรวจ</p>
  {/if}
{/if}

{#if edit}
  <div class="sgbg" on:mousedown|self={() => (edit = null)}>
    <div class="sgbox wide">
      <h3>{edit.id ? `ข้อ ${edit.clause || ''}` : 'เพิ่มข้อที่ตรวจ'}</h3>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ข้อกำหนด</label><input bind:value={edit.clause} placeholder="เช่น 8.5" /></div>
        <div class="sgf"><label>แผนก (ถ้าเฉพาะ)</label><input bind:value={edit.dept} placeholder="เช่น PD" /></div>
      </div>
      <div class="sgf"><label>สิ่งที่ตรวจ <em>*</em></label><textarea rows="2" bind:value={edit.question}></textarea></div>
      <div class="sgf"><label>ผลการตรวจ</label>
        <div class="ia-chips">{#each Object.entries(RESULT) as [k, l]}<button type="button" class="ia-res r-{k}" class:on={edit.result === k} disabled={!!edit.carRef && !['minor', 'major'].includes(k)} on:click={() => (edit.result = k)}>{l}</button>{/each}</div>
      </div>
      <div class="sgf"><label>หลักฐานที่สุ่มตรวจ {#if ['conform', 'ofi', 'minor', 'major'].includes(edit.result)}<em>*</em>{/if}</label><textarea rows="2" bind:value={edit.evidence} placeholder="เอกสาร/บันทึกที่ดู เลขที่ ผู้ที่สัมภาษณ์ ฯลฯ"></textarea></div>
      {#if ['ofi', 'minor', 'major'].includes(edit.result)}
        <div class="sgf"><label>ข้อพบ <em>*</em></label><textarea rows="3" bind:value={edit.finding} placeholder="สิ่งที่พบ เทียบกับข้อกำหนดที่ไม่เป็นไปตาม"></textarea></div>
      {/if}
      {#if err}<div class="merr">{err}</div>{/if}
      <div class="sgact">
        {#if edit.id && !edit.carRef}<button class="mbtn gh text-danger" on:click={() => saveItem(true)}>ลบข้อนี้</button>{/if}
        <span class="flex-1"></span>
        <button class="mbtn gh" on:click={() => (edit = null)}>ยกเลิก</button>
        <button class="mbtn accent" disabled={saving} on:click={() => saveItem(false)}>{saving ? 'กำลังบันทึก…' : 'บันทึก'}</button>
      </div>
    </div>
  </div>
{/if}

{#if dd}
  <DecisionDialog title={dd.title} message="{a.refNo} · {a.title}" {me} commentLabel={dd.comment} commentRequired={!!dd.required}
    withPin={!dd.noPin} confirmLabel={dd.label} danger={!!dd.danger} on:confirm={onDecide} on:cancel={() => (deciding = null)} />
{/if}
