<script>
  // Training matrix — ความสามารถของบุคลากร (ISO 9001:2015 ข้อ 7.2)
  //   ผู้อนุมัติขึ้นไป: matrix ทุกคน · กำหนดข้อกำหนดการอบรม · บันทึกการอบรมนอกระบบ  ·  คนอื่น: สถานะและประวัติของตนเอง
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import { fmtDate } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let me = null
  export let docs = []
  const dispatch = createEventDispatcher()

  const STATUS = { met: 'ผ่าน', expired: 'หมดอายุ', pending: 'รอรับทราบ', outdated: 'ต้องรับทราบฉบับใหม่', missing: 'ยังขาด' }
  const TARGET = { all: 'ทุกคน', dept: 'แผนก', position: 'ตำแหน่ง', emp: 'พนักงาน' }
  const SRC = { form: 'FM-HR-008', ack: 'รับทราบเอกสาร', manual: 'บันทึกตรง' }
  let data = null, err = '', msg = '', view = 'matrix', dept = '', onlyGaps = false
  let person = null, hist = [], editReq = null, rec = null, busy = false, formErr = ''

  async function load() {
    err = ''
    try { data = await api.trainingMatrix(dept); if (data.error) throw new Error(data.error) } catch (e) { err = 'โหลดไม่สำเร็จ: ' + e.message }
  }
  onMount(load)

  $: reqs = data?.requirements || []
  $: rows = (data?.employees || []).filter((e) => !onlyGaps || e.met < e.required)
  $: sum = data?.summary
  const mark = (c) => (c ? { met: '✓', expired: '!', pending: '…', outdated: '↻', missing: '✗' }[c.status] : '')
  const target = (r) => r.targetType === 'all' ? 'ทุกคน' : `${TARGET[r.targetType]} ${r.targetType === 'dept' ? DEPTNAME[r.target] || r.target : r.target}`

  async function openPerson(e) {
    person = e
    try { hist = await api.trainingHistory(e.code) } catch (x) { hist = [] }
  }
  function newReq() { editReq = { id: null, title: '', kind: 'course', keywords: '', docCode: '', targetType: 'all', target: '', validityMonths: 0, clause: '7.2' }; formErr = '' }
  async function saveReq(off = false) {
    busy = true; formErr = ''
    const f = { ...editReq, validityMonths: +editReq.validityMonths || 0 }
    if (off) f.active = false
    const r = editReq.id ? await api.trainingReqUpdate(editReq.id, f) : await api.trainingReqCreate(f)
    busy = false
    if (!r.ok) { formErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    msg = off ? 'ยกเลิกข้อกำหนดแล้ว' : 'บันทึกข้อกำหนดแล้ว'
    editReq = null
    load()
  }
  function newRecord(reqId = null, empCodes = []) {
    rec = { reqId, course: '', date: new Date(Date.now() + 7 * 3600e3).toISOString().slice(0, 10), result: 'ผ่าน', refNo: '', note: '', empCodes, deptPick: '' }
    formErr = ''
  }
  $: pickList = rec ? (data?.employees || []).filter((e) => !rec.deptPick || e.dept === rec.deptPick) : []
  function togglePick(code) { rec.empCodes = rec.empCodes.includes(code) ? rec.empCodes.filter((c) => c !== code) : [...rec.empCodes, code] }
  async function saveRecord() {
    busy = true; formErr = ''
    const r = await api.trainingRecord({ empCodes: rec.empCodes, reqId: rec.reqId ? +rec.reqId : null, course: rec.course, date: rec.date, result: rec.result, refNo: rec.refNo, note: rec.note })
    busy = false
    if (!r.ok) { formErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    msg = `บันทึกการอบรม ${r.body.count} คนแล้ว`
    rec = null
    load()
  }
  function exportCsv() {
    const cell = (v) => { let x = String(v ?? ''); if (/^[\s]*[=+\-@\t\r]/.test(x)) x = "'" + x; return '"' + x.replace(/"/g, '""') + '"' }
    const head = ['รหัส', 'ชื่อ', 'แผนก', 'ตำแหน่ง', 'ผ่าน/ที่ต้องมี', ...reqs.map((r) => r.title)]
    const lines = [head, ...(data?.employees || []).map((e) => [e.code, e.name, e.dept, e.position, `${e.met}/${e.required}`,
      ...reqs.map((r) => { const c = e.cells[r.id]; return c ? `${STATUS[c.status]}${c.date ? ' ' + c.date : ''}${c.expires ? ' (หมดอายุ ' + c.expires + ')' : ''}` : '—' })])]
    const url = URL.createObjectURL(new Blob(['﻿' + lines.map((l) => l.map(cell).join(',')).join('\r\n')], { type: 'text/csv;charset=utf-8' }))
    const a = document.createElement('a'); a.href = url; a.download = 'training-matrix.csv'; a.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="user" size={20} /> Training matrix</h2>
  <p class="cat-desc">ISO 9001:2015 ข้อ 7.2 — กำหนดว่าแต่ละแผนก/ตำแหน่งต้องผ่านการอบรมหรือรับทราบเอกสารอะไร แล้วดูว่าใครยังขาด · ข้อมูลจาก FM-HR-008 การรับทราบเอกสาร และบันทึกการอบรมนอกระบบ</p>
</div>
{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}
{#if msg}<div class="save-ok">{msg}</div>{/if}

{#if data}
  {#if data.canManage}
    <div class="rd-metrics">
      <div class="rd-metric"><div class="rd-ml">ผ่านตามที่กำหนด<Icon name="circle-check" size={18} /></div><div class="rd-mv">{sum.rate ?? 0}%</div><div class="rd-mn"><i class="dot {sum.rate === 100 ? 'ok' : 'warn'}"></i>{sum.met}/{sum.cells} ช่อง</div></div>
      <div class="rd-metric"><div class="rd-ml">พนักงานที่ยังขาด<Icon name="user" size={18} /></div><div class="rd-mv">{sum.peopleWithGaps}</div><div class="rd-mn"><i class="dot {sum.peopleWithGaps ? 'warn' : 'ok'}"></i>ต้องอบรม/รับทราบเพิ่ม</div></div>
      <div class="rd-metric"><div class="rd-ml">หมดอายุ<Icon name="alert-triangle" size={18} /></div><div class="rd-mv">{sum.expired}</div><div class="rd-mn"><i class="dot {sum.expired ? 'bad' : 'ok'}"></i>ต้องอบรมทบทวน</div></div>
      <div class="rd-metric"><div class="rd-ml">ข้อกำหนดการอบรม<Icon name="list-checks" size={18} /></div><div class="rd-mv">{sum.requirements}</div><div class="rd-mn"><i class="dot {sum.requirements ? 'ok' : 'bad'}"></i>หลักสูตร + เอกสาร</div></div>
    </div>
    <div class="subtabs">
      {#each [['matrix', 'Matrix'], ['reqs', `ข้อกำหนดการอบรม (${reqs.length})`]] as [k, l]}<button class="st-btn {view === k ? 'active' : ''}" on:click={() => (view = k)}>{l}</button>{/each}
      <span class="flex-1"></span>
      <button class="mbtn inline-flex items-center gap-1.5" on:click={() => newRecord()}><Icon name="plus" size={14} /> บันทึกการอบรม</button>
    </div>
  {/if}

  {#if view === 'matrix'}
    {#if !reqs.length}
      <div class="empty-state"><Icon name="list-checks" size={34} stroke={1.5} /><p>ยังไม่มีข้อกำหนดการอบรม{data.canManage ? ' — เพิ่มที่แท็บ “ข้อกำหนดการอบรม”' : ''}</p></div>
    {:else}
      {#if data.canManage}
        <div class="controls">
          <select class="ia-year" bind:value={dept} on:change={load}><option value="">ทุกแผนก</option>{#each DEPTS as d}<option value={d}>{DEPTNAME[d]}</option>{/each}</select>
          <label class="issue-toggle"><input type="checkbox" bind:checked={onlyGaps} /> เฉพาะคนที่ยังขาด</label>
          <span class="flex-1"></span>
          <button class="mbtn gh inline-flex items-center gap-1.5" on:click={exportCsv}><Icon name="download" size={14} /> CSV</button>
        </div>
      {/if}
      <div class="sec !p-0 overflow-auto">
        <table class="tm-table">
          <thead><tr><th class="tm-sticky">พนักงาน</th><th>ผ่าน</th>{#each reqs as r}<th class="tm-col" title="{r.title} · {target(r)}{r.validityMonths ? ` · อายุ ${r.validityMonths} เดือน` : ''}"><span>{r.title}</span></th>{/each}</tr></thead>
          <tbody>
            {#each rows as e (e.code)}
              <tr>
                <td class="tm-sticky"><button class="tm-name" on:click={() => openPerson(e)}><b>{e.name}</b><br /><span class="mut">{e.code} · {DEPTNAME[e.dept] || e.dept}{e.position ? ' · ' + e.position : ''}</span></button></td>
                <td class="nowrap"><b class:text-danger={e.met < e.required}>{e.met}/{e.required}</b></td>
                {#each reqs as r}
                  {@const c = e.cells[r.id]}
                  <td class="tm-cell {c ? 'c-' + c.status : 'c-na'}" title={c ? `${STATUS[c.status]} — ${c.detail}${c.date ? ' · ' + c.date : ''}${c.expires ? ' · หมดอายุ ' + c.expires : ''}` : 'ไม่ต้องมี'}>{mark(c)}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="rd-method"><span class="tm-cell c-met">✓</span> ผ่าน · <span class="tm-cell c-expired">!</span> หมดอายุ · <span class="tm-cell c-pending">…</span> รอรับทราบ · <span class="tm-cell c-outdated">↻</span> ต้องรับทราบฉบับใหม่ · <span class="tm-cell c-missing">✗</span> ยังขาด · ช่องว่าง = ไม่ต้องมี · คลิกชื่อเพื่อดูประวัติ</p>
    {/if}
  {:else}
    <div class="flex mb-2"><span class="flex-1"></span><button class="mbtn accent inline-flex items-center gap-1.5" on:click={newReq}><Icon name="plus" size={14} /> เพิ่มข้อกำหนด</button></div>
    {#if reqs.length}
      <div class="sec !p-0 overflow-auto">
        <table class="chg-table">
          <thead><tr><th>ข้อกำหนด</th><th>ชนิด</th><th>กลุ่มเป้าหมาย</th><th>จับคู่</th><th>อายุ</th><th></th></tr></thead>
          <tbody>
            {#each reqs as r (r.id)}
              <tr><td><b>{r.title}</b></td><td>{r.kind === 'doc' ? 'รับทราบเอกสาร' : 'หลักสูตรอบรม'}</td><td>{target(r)}</td>
                <td class="text-[12px]">{r.kind === 'doc' ? r.docCode : r.keywords || '— (บันทึกตรงเท่านั้น)'}</td><td>{r.validityMonths ? r.validityMonths + ' เดือน' : 'ไม่หมดอายุ'}</td>
                <td class="nowrap"><button class="mbtn gh" on:click={() => { editReq = { ...r }; formErr = '' }}>แก้ไข</button>{#if r.kind === 'course'}<button class="mbtn gh" on:click={() => newRecord(r.id)}>บันทึกการอบรม</button>{/if}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}<div class="empty-state"><p>ยังไม่มีข้อกำหนด</p></div>{/if}
  {/if}
{:else if !err}
  <div class="loadwrap"><span class="spin"></span></div>
{/if}

{#if person}
  <div class="sgbg" on:mousedown|self={() => (person = null)}>
    <div class="sgbox wide">
      <h3>{person.name} <span class="mut text-[13px]">{person.code} · {DEPTNAME[person.dept] || person.dept} · {person.position}</span></h3>
      <h4 class="rd-sub !mt-2">สถานะตามข้อกำหนด ({person.met}/{person.required})</h4>
      {#each reqs.filter((r) => person.cells[r.id]) as r}
        {@const c = person.cells[r.id]}
        <div class="cl-ctl flex items-center gap-2"><span class="tm-cell c-{c.status}">{mark(c)}</span><b>{r.title}</b><span class="mut truncate">{c.detail}{c.date ? ' · ' + fmtDate(c.date) : ''}{c.expires ? ' · หมดอายุ ' + fmtDate(c.expires) : ''}</span>
          {#if r.kind === 'doc' && c.status !== 'met'}<button class="mbtn gh ml-auto" on:click={() => { person = null; dispatch('open', r.docCode) }}>เปิดเอกสาร</button>{/if}</div>
      {/each}
      <h4 class="rd-sub">ประวัติการอบรมทั้งหมด ({hist.length})</h4>
      <div class="overflow-auto" style="max-height:300px">
        <table class="chg-table"><thead><tr><th>วันที่</th><th>หลักสูตร</th><th>ผล</th><th>อ้างอิง</th><th>ที่มา</th></tr></thead>
          <tbody>{#each hist as h}<tr><td class="nowrap">{h.date ? fmtDate(h.date) : '—'}</td><td>{h.course}{#if h.note}<br /><span class="mut">{h.note}</span>{/if}</td><td>{h.result}</td><td>{h.ref}</td><td class="nowrap">{SRC[h.source] || h.source}{h.recordedBy ? ' · ' + h.recordedBy : ''}</td></tr>{/each}</tbody></table>
      </div>
      <div class="sgact">{#if data?.canManage}<button class="mbtn" on:click={() => { const c = person.code; person = null; newRecord(null, [c]) }}>บันทึกการอบรมให้คนนี้</button>{/if}<button class="mbtn gh" on:click={() => (person = null)}>ปิด</button></div>
    </div>
  </div>
{/if}

{#if editReq}
  <div class="sgbg" on:mousedown|self={() => (editReq = null)}>
    <div class="sgbox wide">
      <h3>{editReq.id ? 'แก้ไขข้อกำหนดการอบรม' : 'เพิ่มข้อกำหนดการอบรม'}</h3>
      <div class="sgf"><label>ชื่อข้อกำหนด <em>*</em></label><input bind:value={editReq.title} placeholder="เช่น อบรม ISO 9001 Awareness, ความปลอดภัยในการใช้เครื่องจักร" /></div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ชนิด</label><select bind:value={editReq.kind}><option value="course">หลักสูตรอบรม</option><option value="doc">รับทราบเอกสาร</option></select></div>
        <div class="sgf"><label>อายุใบรับรอง (เดือน, 0 = ไม่หมดอายุ)</label><input type="number" min="0" bind:value={editReq.validityMonths} disabled={editReq.kind === 'doc'} /></div>
      </div>
      {#if editReq.kind === 'course'}
        <div class="sgf"><label>คำค้นจับคู่หลักสูตรใน FM-HR-008 (คั่นด้วย ,)</label><input bind:value={editReq.keywords} placeholder="เช่น ISO 9001, awareness" /><p class="mnote !mt-1 !mb-0">ว่างได้ — ถ้าว่างจะนับเฉพาะการอบรมที่บันทึกโดยเลือกข้อกำหนดนี้</p></div>
      {:else}
        <div class="sgf"><label>เอกสารที่ต้องรับทราบ <em>*</em></label><select bind:value={editReq.docCode}><option value="">— เลือก —</option>{#each docs as d}<option value={d.code}>{d.code} · {d.name}</option>{/each}</select></div>
      {/if}
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>กลุ่มเป้าหมาย</label><select bind:value={editReq.targetType}>{#each Object.entries(TARGET) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
        <div class="sgf"><label>{editReq.targetType === 'all' ? '—' : TARGET[editReq.targetType]}</label>
          {#if editReq.targetType === 'dept'}<select bind:value={editReq.target}><option value="">— เลือก —</option>{#each DEPTS as d}<option value={d}>{DEPTNAME[d]}</option>{/each}</select>
          {:else if editReq.targetType === 'position'}<select bind:value={editReq.target}><option value="">— เลือก —</option>{#each data.positions as p}<option value={p}>{p}</option>{/each}</select>
          {:else if editReq.targetType === 'emp'}<input bind:value={editReq.target} placeholder="รหัสพนักงาน" />
          {:else}<input disabled value="ทุกคน" />{/if}
        </div>
      </div>
      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact">{#if editReq.id}<button class="mbtn gh text-danger" disabled={busy} on:click={() => saveReq(true)}>ยกเลิกข้อกำหนดนี้</button>{/if}<span class="flex-1"></span>
        <button class="mbtn gh" on:click={() => (editReq = null)}>ยกเลิก</button><button class="mbtn accent" disabled={busy} on:click={() => saveReq(false)}>บันทึก</button></div>
    </div>
  </div>
{/if}

{#if rec}
  <div class="sgbg" on:mousedown|self={() => (rec = null)}>
    <div class="sgbox wide">
      <h3>บันทึกการอบรม (นอก FM-HR-008)</h3>
      <div class="sgs">เช่น อบรมภายนอก, OJT, ใบรับรองวิชาชีพ — บันทึกโดย {me?.name}</div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ตามข้อกำหนด</label><select bind:value={rec.reqId}><option value={null}>— ไม่ผูก (ระบุหลักสูตรเอง) —</option>{#each reqs.filter((r) => r.kind === 'course') as r}<option value={r.id}>{r.title}</option>{/each}</select></div>
        <div class="sgf"><label>หลักสูตร {#if !rec.reqId}<em>*</em>{/if}</label><input bind:value={rec.course} placeholder={rec.reqId ? 'ใช้ชื่อข้อกำหนด' : 'ชื่อหลักสูตร'} /></div>
        <div class="sgf"><label>วันที่อบรม <em>*</em></label><input type="date" bind:value={rec.date} /></div>
        <div class="sgf"><label>ผล</label><select bind:value={rec.result}><option>ผ่าน</option><option>ไม่ผ่าน</option></select></div>
        <div class="sgf"><label>เลขที่ใบรับรอง/อ้างอิง</label><input bind:value={rec.refNo} /></div>
        <div class="sgf"><label>หมายเหตุ</label><input bind:value={rec.note} /></div>
      </div>
      <div class="sgf"><label>ผู้เข้าอบรม <em>*</em> ({rec.empCodes.length} คน)</label>
        <select class="ia-year mb-2" bind:value={rec.deptPick}><option value="">ทุกแผนก</option>{#each DEPTS as d}<option value={d}>{DEPTNAME[d]}</option>{/each}</select>
        <div class="ia-chips" style="max-height:160px;overflow:auto">{#each pickList as e}<button type="button" class="rd-chip" class:on={rec.empCodes.includes(e.code)} on:click={() => togglePick(e.code)}>{e.name}</button>{/each}</div>
      </div>
      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact"><button class="mbtn gh" on:click={() => (rec = null)}>ยกเลิก</button><button class="mbtn accent" disabled={busy || !rec.empCodes.length} on:click={saveRecord}>บันทึก</button></div>
    </div>
  </div>
{/if}
