<script>
  import { onDestroy, onMount } from 'svelte'
  import * as XLSX from 'xlsx'
  import { api, bytesToB64 } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import Icon from './Icon.svelte'

  export let launch = null
  export let me = null

  const STATUS = {
    draft: 'ร่าง', review: 'รอตรวจสอบ', approve: 'รออนุมัติ', approved: 'อนุมัติแล้ว',
    returned: 'ตีกลับ', cancelled: 'ยกเลิก', imported: 'นำเข้าจากเอกสารเดิม', superseded: 'ถูกแทนที่',
  }
  const MONTHS = ['', 'ม.ค.', 'ก.พ.', 'มี.ค.', 'เม.ย.', 'พ.ค.', 'มิ.ย.', 'ก.ค.', 'ส.ค.', 'ก.ย.', 'ต.ค.', 'พ.ย.', 'ธ.ค.']
  const thDate = (iso) => {
    if (!iso || !iso.includes('-')) return iso || '—'
    const [y, m, d] = iso.split('-')
    return `${Number(d)} ${MONTHS[Number(m)] || ''} ${Number(y) + 543}`
  }
  // หน้ากรอกแบบฟอร์ม: ความกว้างช่อง (จาก 6 คอลัมน์) และคลาสคอลัมน์ตาราง
  const FIELD_SPAN = { date: 2, time: 1, select_or_text: 2, text: 3, radio: 6, checkbox: 6 }
  const COL_CLASS = { employee: 'w-[110px]', signature: 'c w-[130px]', passfail: 'c w-[110px]', text: 'ef-txt' }
  const stepLabel = (s) => ({
    submit: 'ส่งอนุมัติ', return: 'ตีกลับ', cancel: 'ยกเลิก', amend: 'ขอแก้ไขบันทึก', import: 'นำเข้าจากเอกสารเดิม', create: 'สร้างบันทึก',
    pass: s.step === 'approve' ? 'อนุมัติ' : 'ตรวจสอบผ่าน',
  }[s.action] || s.action)
  const canWrite = () => me && me.role !== 'viewer'
  const isAdmin = () => me?.role === 'admin'

  let view = 'list'
  let schemas = []
  let rows = []
  let stats = { thisMonth: 0, pending: 0, overdue: 0 }
  let err = ''
  let loading = true
  let filter = { form: '', status: '', q: '' }
  let rec = null
  let data = {}
  let schema = null
  let dirty = false
  let msg = ''
  let places = []
  let fileInput
  let reason = ''
  let showReason = ''
  let training = []
  let tq = ''
  let csv = ''
  let schemaText = ''
  let pinOld = ''
  let pinNew = ''
  let seenLaunch = 0
  let sigRow = -1
  let canvas

  async function refreshLists() {
    const q = {}
    if (filter.form) q.form = filter.form
    if (filter.status) q.status = filter.status
    if (filter.q) q.q = filter.q
    ;[schemas, rows, stats] = await Promise.all([
      api.schemas(),
      view === 'pending' ? api.formPending().then((p) => p.items || []) : api.formRecords(q),
      api.formStats(),
    ])
  }

  async function load() {
    loading = true; err = ''
    try { await refreshLists() } catch (e) { err = e.message }
    loading = false
  }
  onMount(load)

  async function openLaunch(l) {
    filter = { ...filter, form: l.code || '' }
    if (l.mode === 'list') { view = 'list'; rec = null; await load(); return }
    if (!canWrite()) { err = 'สิทธิ์นี้ดูได้อย่างเดียว'; return }
    view = 'list'
    const { ok, body } = await api.formCreate(l.code)
    if (!ok) { err = body.error || 'เปิดฟอร์มไม่ได้'; return }
    openRec(body)
  }
  $: if (launch && launch.n && launch.n !== seenLaunch) { seenLaunch = launch.n; openLaunch(launch) }

  function blankFrom(sc) {
    const d = { participants: [] }
    for (const f of sc.header_fields || []) {
      d[f.key] = f.type === 'checkbox' ? [] : ''
      if (f.otherKey) d[f.otherKey] = ''
    }
    return d
  }

  async function openRec(body) {
    rec = body
    schema = body.schema
    data = body.data && Object.keys(body.data).length ? body.data : blankFrom(schema || {})
    if (!data.participants) data.participants = []
    dirty = false
    view = editable() ? 'edit' : 'view'
    places = await api.formPlaces(body.formCode).catch(() => [])
    msg = ''
  }

  async function openId(id) {
    err = ''
    try { openRec(await api.formGet(id)) } catch (e) { err = e.message }
  }

  function editable() {
    return rec && (rec.status === 'draft' || rec.status === 'returned') && (isAdmin() || rec.preparedBy === me?.empCode)
  }

  async function save(silent) {
    if (!rec || !editable()) return
    const { ok, body } = await api.formSave(rec.id, data)
    if (!ok) { if (!silent) err = body.error || 'บันทึกไม่ได้'; return }
    dirty = false
    if (!silent) msg = 'บันทึกร่างแล้ว'
  }

  const timer = setInterval(() => { if (dirty && view === 'edit') save(true) }, 30000)
  onDestroy(() => clearInterval(timer))

  function setField(key, value) { data = { ...data, [key]: value }; dirty = true; msg = '' }
  function toggleCheck(key, value) {
    const cur = Array.isArray(data[key]) ? data[key] : []
    const next = cur.includes(value) ? cur.filter((x) => x !== value) : [...cur, value]
    setField(key, next)
  }

  function addRow() {
    data.participants = [...(data.participants || []), { empCode: '', name: '', position: '', dept: '', signature: '', result: '', note: '', attendedAt: '' }]
    data = data; dirty = true
  }
  function delRow(i) { data.participants = data.participants.filter((_, n) => n !== i); data = data; dirty = true }

  async function onEmp(i) {
    const code = (data.participants[i].empCode || '').trim()
    if (!code || code === '-') return
    const dup = data.participants.some((p, n) => n !== i && (p.empCode || '').trim() === code)
    if (dup) { err = `รหัส ${code} ซ้ำในใบนี้`; return }
    const list = await api.employees(code).catch(() => [])
    const hit = list.find((e) => e.empCode === code)
    if (!hit) return
    data.participants[i] = { ...data.participants[i], name: hit.nameTh, position: hit.position, dept: DEPTNAME[hit.dept] || hit.dept }
    data = data; dirty = true; err = ''
  }

  async function addDept(dept) {
    const list = await api.employees('', dept)
    const have = new Set((data.participants || []).map((p) => (p.empCode || '').trim()))
    const add = list.filter((e) => e.empCode && !have.has(e.empCode)).map((e) => ({
      empCode: e.empCode, name: e.nameTh, position: e.position, dept: DEPTNAME[e.dept] || e.dept, signature: '', result: '', note: '', attendedAt: '',
    }))
    data.participants = [...(data.participants || []), ...add]
    data = data; dirty = true
  }

  async function copyPrev() {
    const prev = await api.formPrevious(rec.formCode)
    data.participants = prev.participants || []
    data = data; dirty = true
  }

  async function submitRec() {
    err = ''
    await save(true)
    const { ok, body } = await api.formSubmit(rec.id)
    if (!ok) {
      const m = body.error || 'ส่งไม่สำเร็จ'
      // แบบฟอร์มออก Rev. ใหม่ระหว่างกรอก — โหลดใบใหม่ให้เห็นแถบย้ายฉบับ
      if (body.staleForm) await openId(rec.id)
      err = m
      return
    }
    openRec(body)
    msg = 'ส่งอนุมัติแล้ว ' + body.refNo
  }

  /** ย้ายร่างไปแบบฟอร์มฉบับใช้งานปัจจุบัน (ข้อมูลที่กรอกคงเดิม) */
  async function upgradeRec() {
    err = ''
    await save(true)
    const { ok, body } = await api.formUpgrade(rec.id)
    if (!ok) { err = body.error || 'ย้ายฉบับไม่สำเร็จ'; return }
    await openRec(body)
    msg = `ย้ายไปแบบฟอร์ม ${body.formRev} แล้ว — ตรวจข้อมูลแล้วจึงส่ง`
  }

  async function act(action) {
    if (action === 'return' && !reason.trim()) { err = 'กรุณาระบุเหตุผล'; return }
    const { ok, body } = await api.formAct(rec.id, action, reason)
    if (!ok) { err = body.error || 'ทำรายการไม่ได้'; return }
    reason = ''; openRec(body)
  }

  async function doReason() {
    const fn = showReason === 'cancel' ? api.formCancel : api.formAmend
    const { ok, body } = await fn(rec.id, reason)
    if (!ok) { err = body.error || 'ทำรายการไม่ได้'; return }
    showReason = ''; reason = ''; openRec(body)
  }

  async function onFile(e) {
    const f = e.target.files?.[0]
    e.target.value = ''
    if (!f) return
    const buf = new Uint8Array(await f.arrayBuffer())
    const { ok, body } = await api.formFile(rec.id, f.name, bytesToB64(buf))
    if (!ok) { err = body.error || 'แนบไฟล์ไม่ได้'; return }
    openRec(await api.formGet(rec.id))
  }

  async function printRec() {
    const html = await api.formPrint(rec.id)
    const w = window.open('', '_blank')
    if (!w) { err = 'เบราว์เซอร์บล็อกหน้าต่างพิมพ์'; return }
    w.document.write(html)
    w.document.close()
    w.focus()
    // รอฟอนต์ (Sarabun) และรูป (โลโก้/ลายเซ็น) โหลดครบก่อนสั่งพิมพ์
    const ready = Promise.all([
      w.document.fonts?.ready,
      ...[...w.document.images].map((im) => (im.complete ? null : new Promise((ok) => { im.onload = im.onerror = ok }))),
    ])
    Promise.race([ready, new Promise((ok) => setTimeout(ok, 2500))]).then(() => w.print())
  }

  function exportXlsx() {
    const sheet = rows.map((r) => ({
      เลขที่: r.refNo, แบบฟอร์ม: r.formCode, เรื่อง: r.title, สถานะ: STATUS[r.status] || r.status,
      วันที่อบรม: r.trainDate || '', สถานที่: r.place || '', ผู้จัดทำ: r.preparedName, แผนก: r.dept,
    }))
    const wb = XLSX.utils.book_new()
    XLSX.utils.book_append_sheet(wb, XLSX.utils.json_to_sheet(sheet), 'บันทึก')
    XLSX.writeFile(wb, 'form-records.xlsx')
  }

  async function loadTraining() {
    training = await api.formTraining(tq)
  }

  async function importCsv() {
    const { ok, body } = await api.employeeSave({ csv })
    msg = ok ? `นำเข้า ${body.count} คน` : (body.error || 'นำเข้าไม่ได้')
  }
  async function importSchema() {
    let parsed
    try { parsed = JSON.parse(schemaText) } catch { msg = 'JSON ไม่ถูกต้อง'; return }
    const { ok, body } = await api.schemaSave(parsed)
    msg = ok ? 'บันทึก schema แล้ว' : (body.error || 'บันทึกไม่ได้')
    if (ok) schemas = await api.schemas()
  }
  async function changePin() {
    const { ok, body } = await api.changePin(pinOld, pinNew)
    msg = ok ? 'เปลี่ยน PIN แล้ว กรุณาเข้าสู่ระบบใหม่' : (body.error || 'เปลี่ยนไม่ได้')
    if (ok) { localStorage.removeItem('isoToken'); location.reload() }
  }

  function startSign(i) {
    sigRow = i
    setTimeout(() => {
      if (!canvas) return
      const ctx = canvas.getContext('2d')
      ctx.fillStyle = '#fff'
      ctx.fillRect(0, 0, canvas.width, canvas.height)
      let draw = false
      canvas.onpointerdown = (ev) => { draw = true; ctx.beginPath(); ctx.moveTo(ev.offsetX, ev.offsetY) }
      canvas.onpointermove = (ev) => { if (!draw) return; ctx.lineTo(ev.offsetX, ev.offsetY); ctx.stroke() }
      canvas.onpointerup = () => { draw = false }
    }, 30)
  }
  function keepSign() {
    if (sigRow < 0 || !canvas) return
    data.participants[sigRow].signature = canvas.toDataURL('image/png')
    data = data; dirty = true; sigRow = -1
  }

  function fieldText(f) {
    const v = data[f.key]
    const other = f.otherKey && data[f.otherKey] ? ` (${data[f.otherKey]})` : ''
    if (f.type === 'date') return v ? thDate(v) : ''
    if (f.type === 'checkbox') return (v || []).map((x) => optLabel(f, x)).join(', ') + other
    if (f.type === 'radio') return v ? optLabel(f, v) + other : ''
    return v || ''
  }

  function optLabel(field, value) {
    return (field.options || []).find((o) => (o.value || o) === value)?.label || value
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="clipboard-list" size={20} /> บันทึกแบบฟอร์ม</h2>
  <p class="cat-desc">กรอกแบบฟอร์มควบคุมแยกจากไฟล์ต้นฉบับ · เลขที่ออกตอนส่งอนุมัติ · ผู้จัดทำมาจากบัญชีที่เข้าสู่ระบบ</p>
</div>

<div class="type-tabs">
  <button class="type-tab {view === 'list' ? 'active' : ''}" on:click={() => { view = 'list'; rec = null; load() }}>รายการ</button>
  <button class="type-tab {view === 'pending' ? 'active' : ''}" on:click={() => { view = 'pending'; rec = null; load() }}>งานรอฉัน <span class="tt-n">{stats.pending}</span></button>
  <button class="type-tab" on:click={() => { view = 'training'; loadTraining() }}>ประวัติอบรม FM-HR-007</button>
  {#if isAdmin()}<button class="type-tab {view === 'admin' ? 'active' : ''}" on:click={() => (view = 'admin')}>จัดการ</button>{/if}
  <span class="flex-1"></span>
  {#if view === 'list' && canWrite()}
    <select class="dept-chip" on:change={(e) => { const c = e.currentTarget.value; if (c) openLaunch({ code: c, mode: 'new', n: Date.now() }); e.currentTarget.value = '' }}>
      <option value="">กรอกแบบฟอร์มใหม่</option>
      {#each schemas as s}<option value={s.formCode}>{s.formCode} · {s.title}</option>{/each}
    </select>
  {/if}
</div>

<div class="dept-chips">
  <span class="dept-chip">เดือนนี้ {stats.thisMonth}</span>
  <span class="dept-chip">รออนุมัติ {stats.pending}</span>
  <span class="dept-chip">ค้างเกิน 7 วัน {stats.overdue}</span>
</div>

{#if err}<div class="merr"><span>{err}</span></div>{/if}
{#if msg}<div class="save-ok">{msg}</div>{/if}

{#if view === 'list' || view === 'pending'}
  {#if view === 'list'}
    <div class="grid gap-2 md:grid-cols-4 mb-3">
      <input class="border border-line rounded-lg px-3 py-2" placeholder="ค้นหาเลขที่ / เรื่อง / ผู้จัดทำ" bind:value={filter.q} on:change={load} />
      <select class="border border-line rounded-lg px-3 py-2" bind:value={filter.form} on:change={load}>
        <option value="">ทุกแบบฟอร์ม</option>
        {#each schemas as s}<option value={s.formCode}>{s.formCode}</option>{/each}
      </select>
      <select class="border border-line rounded-lg px-3 py-2" bind:value={filter.status} on:change={load}>
        <option value="">ทุกสถานะ</option>
        {#each Object.entries(STATUS) as [k, lab]}<option value={k}>{lab}</option>{/each}
      </select>
      <button class="mbtn gh" on:click={exportXlsx}>ส่งออก Excel</button>
    </div>
  {/if}
  {#if loading}
    <div class="loadwrap"><span class="spin"></span></div>
  {:else if !rows.length}
    <div class="empty-state"><p>ยังไม่มีบันทึก</p></div>
  {:else}
    <div class="act-grid">
      {#each rows as r (r.id)}
        <div class="act-card" role="button" tabindex="0" on:click={() => openId(r.id)} on:keydown={(e) => e.key === 'Enter' && openId(r.id)}>
          <div class="ac-top"><span class="ac-ref">{r.refNo}</span><span class="flex-1"></span><span class="status-badge">{STATUS[r.status] || r.status}</span></div>
          <div class="ac-title">{r.title || r.formCode}</div>
          <div class="ac-meta">
            <span>{r.formCode}</span>
            {#if r.trainDate}<span>{thDate(r.trainDate)}</span>{/if}
            <span>{r.preparedName}</span>
            {#if r.revNo > 1}<span>แก้ครั้งที่ {r.revNo}</span>{/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
{:else if view === 'edit' || view === 'view'}
  {#if rec}
    <div class="flex flex-wrap items-center gap-2 mb-3">
      <button class="mbtn gh" on:click={() => { view = 'list'; rec = null; load() }}>กลับรายการ</button>
      <b>{rec.refNo}</b>
      <span class="status-badge">{STATUS[rec.status]}</span>
      {#if rec.revNo > 1}<span class="note">ฉบับแก้ครั้งที่ {rec.revNo}</span>{/if}
      <span class="flex-1"></span>
      <button class="mbtn gh inline-flex items-center gap-1" on:click={printRec}><Icon name="printer" size={14} /> พิมพ์ / PDF</button>
    </div>
    <div class="note mb-2">ผู้จัดทำ {rec.preparedName} · {rec.preparedPosition || ''} · แก้ชื่อผู้จัดทำไม่ได้</div>
    {#if rec.status === 'returned'}
      <div class="merr">ตีกลับ: {rec.steps?.filter((s) => s.action === 'return').slice(-1)[0]?.comment || ''}</div>
    {/if}
    {#if rec.staleForm}
      <div class="ver-warn">
        <Icon name="alert-triangle" size={16} />
        <span>ใบนี้กรอกบนแบบฟอร์ม <b>{rec.formRev}</b> แต่ฉบับใช้งานปัจจุบันคือ <b>{rec.currentFormRev}</b> — ส่งอนุมัติไม่ได้จนกว่าจะย้ายไปฉบับปัจจุบัน (ข้อมูลที่กรอกไว้ยังอยู่ ตรวจช่องที่เปลี่ยนแล้วจึงส่ง)</span>
        {#if editable()}<button class="vw-btn" on:click={upgradeRec}>ย้ายไป {rec.currentFormRev}</button>{/if}
      </div>
    {/if}

    {#if schema}
      <div class="ef-paper">
        <header class="ef-head">
          <img class="ef-logo" src="/kg-logo.jpg" alt="K Garden" />
          <div class="ef-title">
            <div class="ef-org">บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด</div>
            <h2>{schema.title}</h2>
            <div class="ef-ref">เลขที่ <b>{rec.refNo}</b></div>
          </div>
          <div class="ef-code">
            <b>{schema.form_code}</b>
            <span>{schema.form_rev}</span>
            <span>บังคับใช้ {schema.effective}</span>
          </div>
        </header>

        <div class="ef-fields">
          {#each schema.header_fields || [] as f}
            <div class="ef-field" style="grid-column: span {FIELD_SPAN[f.type] || 3}">
              <span class="ef-label">{f.label}{#if f.required && editable()} <em>*</em>{/if}</span>
              {#if !editable()}
                <div class="ef-val">{fieldText(f) || '—'}</div>
              {:else if f.type === 'date'}
                <input class="ef-in" type="date" value={data[f.key] || ''} on:input={(e) => setField(f.key, e.currentTarget.value)} />
              {:else if f.type === 'time'}
                <input class="ef-in" type="time" value={data[f.key] || ''} on:input={(e) => setField(f.key, e.currentTarget.value)} />
              {:else if f.type === 'select_or_text'}
                <input class="ef-in" list="places-{f.key}" placeholder="เลือกหรือพิมพ์" value={data[f.key] || ''} on:input={(e) => setField(f.key, e.currentTarget.value)} />
                <datalist id="places-{f.key}">{#each [...(f.options || []), ...places] as o}<option value={o}></option>{/each}</datalist>
              {:else if f.type === 'radio' || f.type === 'checkbox'}
                <div class="ef-opts">
                  {#each f.options as o}
                    {@const on = f.type === 'radio' ? data[f.key] === o.value : (data[f.key] || []).includes(o.value)}
                    <label class="ef-chip" class:on>
                      {#if f.type === 'radio'}
                        <input type="radio" name={f.key} checked={on} on:change={() => setField(f.key, o.value)} />
                      {:else}
                        <input type="checkbox" checked={on} on:change={() => toggleCheck(f.key, o.value)} />
                      {/if}
                      <span class="ef-mark {f.type}"></span>{o.label}
                    </label>
                  {/each}
                  {#if f.type === 'radio' ? data[f.key] === 'other' : (data[f.key] || []).includes('other')}
                    <input class="ef-in ef-other" placeholder="ระบุ" value={data[f.otherKey] || ''} on:input={(e) => setField(f.otherKey, e.currentTarget.value)} />
                  {/if}
                </div>
              {:else}
                <input class="ef-in" value={data[f.key] || ''} on:input={(e) => setField(f.key, e.currentTarget.value)} />
              {/if}
            </div>
          {/each}
        </div>

        {#each schema.table_sections || [] as sec}
          <div class="ef-sec">
            <div class="ef-sec-h">
              <b>{sec.label}</b><span class="ef-count">{(data.participants || []).length} คน</span>
              <span class="flex-1"></span>
              {#if editable()}
                {#if sec.deptFill}
                  <select class="ef-in ef-sm" on:change={(e) => { if (e.currentTarget.value) addDept(e.currentTarget.value); e.currentTarget.value = '' }}>
                    <option value="">+ เพิ่มทั้งแผนก</option>
                    {#each DEPTS as d}<option value={d}>{DEPTNAME[d]}</option>{/each}
                  </select>
                {/if}
                {#if sec.copyPrevious}<button class="mbtn gh" on:click={copyPrev}>คัดลอกรายชื่อครั้งก่อน</button>{/if}
                <button class="mbtn accent inline-flex items-center gap-1" on:click={addRow}><Icon name="plus" size={14} /> เพิ่มแถว</button>
              {/if}
            </div>
            <div class="overflow-x-auto">
              <table class="ef-table">
                <thead><tr>
                  <th class="c w-[46px]">ลำดับ</th>
                  {#each sec.columns as c}<th class={COL_CLASS[c.type] || ''}>{c.label}</th>{/each}
                  {#if editable()}<th class="w-[40px]"></th>{/if}
                </tr></thead>
                <tbody>
                  {#each data.participants || [] as row, i}
                    <tr>
                      <td class="c">{i + 1}</td>
                      {#each sec.columns as c}
                        <td class={COL_CLASS[c.type] || ''}>
                          {#if !editable()}
                            {#if c.type === 'signature'}
                              {#if row.signature}<img alt="ลายมือชื่อ" src={row.signature} class="ef-sig" />{:else if row.attendedAt}<span class="ef-ok">เช็คอินแล้ว</span>{:else}—{/if}
                            {:else if c.type === 'passfail'}
                              {#if row.result === 'pass'}<span class="ef-pass">✓ ผ่าน</span>{:else if row.result === 'fail'}<span class="ef-fail">✗ ไม่ผ่าน</span>{:else}—{/if}
                            {:else}{row[c.key] || ''}{/if}
                          {:else if c.type === 'employee'}
                            <input class="ef-cell" placeholder="รหัส" bind:value={row.empCode} on:change={() => onEmp(i)} />
                          {:else if c.type === 'passfail'}
                            <select class="ef-cell" bind:value={row.result} on:change={() => (dirty = true)}><option value="">—</option><option value="pass">ผ่าน</option><option value="fail">ไม่ผ่าน</option></select>
                          {:else if c.type === 'signature'}
                            <button class="ef-signbtn" class:done={row.signature || row.attendedAt} on:click={() => startSign(i)}>
                              {#if row.signature}<img alt="" src={row.signature} class="ef-sig" />{:else if row.attendedAt}เช็คอินแล้ว{:else}<Icon name="pen-line" size={13} /> เซ็นชื่อ{/if}
                            </button>
                          {:else}
                            <input class="ef-cell" bind:value={row[c.key]} on:input={() => (dirty = true)} />
                          {/if}
                        </td>
                      {/each}
                      {#if editable()}<td class="c"><button class="ef-del" title="ลบแถว" on:click={() => delRow(i)}><Icon name="x" size={14} /></button></td>{/if}
                    </tr>
                  {:else}
                    <tr><td class="ef-empty" colspan={sec.columns.length + (editable() ? 2 : 1)}>ยังไม่มีรายชื่อ{#if editable()} — กด “เพิ่มแถว” หรือ “เพิ่มทั้งแผนก”{/if}</td></tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </div>
        {/each}

        <footer class="ef-foot">{schema.form_code} {schema.form_rev} · วันที่มีผลบังคับใช้ {schema.effective}</footer>
      </div>
    {/if}

    <div class="ef-side">
      <section class="ef-card">
        <h4><Icon name="note" size={14} /> ไฟล์แนบ</h4>
        {#if (rec.files || []).length}
          <ul class="ef-files">{#each rec.files as f}<li><a href={`/api/forms/files/${f.id}`} on:click|preventDefault={async () => {
            const r = await fetch(`/api/forms/files/${f.id}`, { headers: { Authorization: 'Bearer ' + (localStorage.getItem('isoToken') || '') } })
            const b = await r.blob(); const u = URL.createObjectURL(b); const a = document.createElement('a'); a.href = u; a.download = f.filename; a.click()
          }}>{f.filename}</a></li>{/each}</ul>
        {:else}<p class="note">ไม่มีไฟล์แนบ</p>{/if}
        {#if editable()}<button class="mbtn gh mt-2" on:click={() => fileInput.click()}>แนบไฟล์</button><input bind:this={fileInput} type="file" class="hidden" on:change={onFile} />{/if}
      </section>

      {#if rec.checkinUrl && editable() || (rec.status === 'review' || rec.status === 'approve' || rec.status === 'approved')}
        <section class="ef-card">
          <h4><Icon name="layout-grid" size={14} /> QR เช็คอินผู้เข้าอบรม</h4>
          <div class="w-32">{@html rec.checkinQr || ''}</div>
          <div class="note break-all mt-1">{rec.checkinUrl}</div>
        </section>
      {/if}

      <section class="ef-card">
        <h4><Icon name="clock" size={14} /> ขั้นตอนการอนุมัติ</h4>
        {#if (rec.steps || []).length}
          <ol class="ef-steps">
            {#each rec.steps as s}
              <li class="s-{s.action}"><b>{stepLabel(s)}</b> · {s.actorName}<span class="note"> · {s.at?.slice(0, 16).replace('T', ' ')}</span>{#if s.comment}<div class="note">“{s.comment}”</div>{/if}</li>
            {/each}
          </ol>
        {:else}<p class="note">ยังไม่ได้ส่งอนุมัติ</p>{/if}
        {#if (rec.history || []).length > 1}
          <div class="mt-2 flex flex-wrap gap-1">{#each rec.history as h}<button class="mbtn gh" on:click={() => openId(h.id)}>{h.refNo} ครั้งที่ {h.revNo} · {STATUS[h.status]}</button>{/each}</div>
        {/if}
      </section>
    </div>

    <div class="sgact mt-3">
      {#if editable()}
        <button class="mbtn gh" on:click={() => save(false)}>บันทึกร่าง</button>
        <button class="mbtn accent" on:click={submitRec}>ส่งอนุมัติ</button>
      {/if}
      {#if (rec.status === 'review' && me.role !== 'preparer' && me.role !== 'viewer' && (isAdmin() || rec.preparedBy !== me.empCode)) || (rec.status === 'approve' && (me.role === 'approver' || isAdmin()) && (isAdmin() || rec.preparedBy !== me.empCode))}
        <input class="ef-in max-w-[360px]" placeholder="ความเห็น / เหตุผลตีกลับ" bind:value={reason} />
        <button class="mbtn accent" on:click={() => act('pass')}>{rec.status === 'review' ? 'ตรวจผ่าน' : 'อนุมัติ'}</button>
        <button class="mbtn danger" on:click={() => act('return')}>ตีกลับ</button>
      {/if}
      {#if rec.status === 'approved' && (isAdmin() || rec.preparedBy === me.empCode)}
        <button class="mbtn gh" on:click={() => (showReason = 'amend')}>ขอแก้ไขบันทึก</button>
      {/if}
      {#if !['cancelled', 'imported', 'superseded', 'approved'].includes(rec.status) && (isAdmin() || rec.preparedBy === me.empCode)}
        <button class="mbtn danger" on:click={() => (showReason = 'cancel')}>ยกเลิก</button>
      {/if}
    </div>
  {/if}
{:else if view === 'training'}
  <div class="flex gap-2 mb-3">
    <input class="border border-line rounded-lg px-3 py-2" placeholder="ค้นชื่อ / หลักสูตร / เลขที่" bind:value={tq} />
    <button class="mbtn gh" on:click={loadTraining}>ค้นหา</button>
  </div>
  <table class="w-full text-[13px]">
    <thead><tr><th>วันที่</th><th>ชื่อ</th><th>หลักสูตร</th><th>ผล</th><th>เลขที่</th></tr></thead>
    <tbody>
      {#each training as t}
        <tr class={t.result === 'fail' ? 'bg-danger/10' : ''}>
          <td>{thDate(t.trainDate)}</td><td>{t.name}</td><td>{t.course}</td>
          <td>{t.result === 'fail' ? 'ไม่ผ่าน' : t.result === 'pass' ? 'ผ่าน' : '—'}</td>
          <td><button class="mbtn gh" on:click={() => openId(t.recordId)}>{t.refNo}</button></td>
        </tr>
      {/each}
    </tbody>
  </table>
  <p class="note">แถวพื้นแดงคือผลไม่ผ่าน ต้องติดตามอบรมซ้ำ</p>
{:else if view === 'admin'}
  <div class="grid gap-4 md:grid-cols-2">
    <div>
      <h3>นำเข้าพนักงาน (CSV)</h3>
      <p class="note">emp_code,name_th,name_en,position,dept,role,pin</p>
      <textarea rows="6" class="w-full border border-line rounded-lg p-2" bind:value={csv}></textarea>
      <button class="mbtn accent" on:click={importCsv}>นำเข้า</button>
    </div>
    <div>
      <h3>schema แบบฟอร์ม (JSON)</h3>
      <textarea rows="6" class="w-full border border-line rounded-lg p-2" bind:value={schemaText}></textarea>
      <button class="mbtn accent" on:click={importSchema}>บันทึก schema</button>
    </div>
  </div>
  <h3 class="mt-4">เปลี่ยน PIN ของฉัน</h3>
  <div class="flex gap-2">
    <input type="password" maxlength="6" placeholder="PIN เดิม" bind:value={pinOld} />
    <input type="password" maxlength="6" placeholder="PIN ใหม่" bind:value={pinNew} />
    <button class="mbtn gh" on:click={changePin}>เปลี่ยน</button>
  </div>
{/if}

{#if showReason}
  <div class="sgbg" on:mousedown|self={() => (showReason = '')}>
    <div class="sgbox" style="max-width:460px">
      <h3>{showReason === 'cancel' ? 'ยกเลิกบันทึก' : 'ขอแก้ไขบันทึก'}</h3>
      <div class="sgf"><label>เหตุผล <em>*</em></label><textarea rows="3" bind:value={reason}></textarea></div>
      <div class="sgact">
        <button class="mbtn gh" on:click={() => (showReason = '')}>ปิด</button>
        <button class="mbtn accent" on:click={doReason}>ยืนยัน</button>
      </div>
    </div>
  </div>
{/if}

{#if sigRow >= 0}
  <div class="sgbg" on:mousedown|self={() => (sigRow = -1)}>
    <div class="sgbox" style="max-width:520px">
      <h3>เซ็นชื่อผู้เข้าอบรม</h3>
      <canvas bind:this={canvas} width="440" height="160" class="border border-line w-full touch-none"></canvas>
      <div class="sgact">
        <button class="mbtn gh" on:click={() => (sigRow = -1)}>ปิด</button>
        <button class="mbtn accent" on:click={keepSign}>ใช้ลายเซ็นนี้</button>
      </div>
    </div>
  </div>
{/if}

<style>
  table { border-collapse: collapse; }
  th, td { border-bottom: 1px solid #E6EAF0; text-align: left; padding: 6px; vertical-align: top; }
  input, select, textarea { font: inherit; }
  .sgf input, .sgf select { min-height: 42px; }

  /* ---------- หน้ากรอกแบบฟอร์ม: จัดเป็นแผ่นแบบฟอร์มเหมือนต้นฉบับ ---------- */
  .ef-paper { background: #fff; border: 1px solid #dfe5ee; border-radius: 14px; box-shadow: 0 2px 10px rgba(16, 38, 77, .06); padding: 22px 26px 16px; max-width: 1100px; }
  .ef-head { display: grid; grid-template-columns: 64px 1fr auto; gap: 16px; align-items: center; padding-bottom: 14px; border-bottom: 2px solid #0B3679; }
  .ef-logo { width: 60px; height: 58px; object-fit: contain; }
  .ef-title { text-align: center; }
  .ef-org { font-size: 12px; color: #5b6b85; }
  .ef-title h2 { margin: 2px 0 2px; font-size: 21px; font-weight: 700; color: #0B3679; }
  .ef-ref { font-size: 12.5px; color: #41506a; }
  .ef-code { display: flex; flex-direction: column; align-items: flex-end; gap: 1px; font-size: 12px; color: #41506a; border: 1px solid #dfe5ee; border-radius: 10px; padding: 6px 12px; background: #f7f9fc; }
  .ef-code b { color: #0B3679; font-size: 13.5px; }

  .ef-fields { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 12px 16px; padding: 16px 0 6px; }
  .ef-field { display: flex; flex-direction: column; gap: 5px; min-width: 0; }
  .ef-label { font-size: 12.5px; font-weight: 700; color: #41506a; }
  .ef-label em { color: #c0392b; font-style: normal; }
  .ef-in { width: 100%; min-height: 40px; padding: 8px 11px; border: 1px solid #cdd4df; border-radius: 9px; background: #fff; font-size: 13.5px; color: #1a2433; }
  .ef-in:focus, .ef-cell:focus { outline: none; border-color: #0B3679; box-shadow: 0 0 0 3px rgba(11, 54, 121, .12); }
  .ef-val { min-height: 32px; padding: 6px 2px; border-bottom: 1px dashed #c3ccd9; font-size: 14px; color: #1a2433; }

  .ef-opts { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .ef-chip { position: relative; display: inline-flex; align-items: center; gap: 8px; padding: 8px 14px; border: 1px solid #cdd4df; border-radius: 999px; background: #fff; font-size: 13.5px; cursor: pointer; user-select: none; transition: background .12s, border-color .12s; }
  .ef-chip:hover { border-color: #0B3679; }
  .ef-chip input { position: absolute; opacity: 0; width: 0; height: 0; }
  .ef-chip:focus-within { box-shadow: 0 0 0 3px rgba(11, 54, 121, .15); }
  .ef-mark { width: 16px; height: 16px; border: 1.5px solid #8a97ab; background: #fff; flex-shrink: 0; display: inline-flex; align-items: center; justify-content: center; }
  .ef-mark.radio { border-radius: 50%; }
  .ef-mark.checkbox { border-radius: 4px; }
  .ef-chip.on { background: #eaf1fb; border-color: #0B3679; color: #0B3679; font-weight: 700; }
  .ef-chip.on .ef-mark { border-color: #0B3679; background: #0B3679; }
  .ef-chip.on .ef-mark.radio { box-shadow: inset 0 0 0 3px #fff; }
  .ef-chip.on .ef-mark.checkbox::after { content: ''; width: 5px; height: 9px; border: solid #fff; border-width: 0 2px 2px 0; transform: rotate(45deg) translate(-1px, -1px); }
  .ef-other { width: 220px; min-height: 36px; }

  .ef-sec { margin-top: 18px; }
  .ef-sec-h { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-bottom: 8px; }
  .ef-sec-h b { font-size: 15px; color: #0B3679; }
  .ef-count { font-size: 12px; color: #5b6b85; background: #eef2f7; border-radius: 999px; padding: 2px 9px; }
  .ef-sm { width: auto; min-height: 36px; padding: 6px 10px; }
  .ef-table { width: 100%; border-collapse: collapse; font-size: 13px; border: 1px solid #c9d2df; }
  .ef-table th { background: #eaf1fb; color: #0B3679; font-weight: 700; padding: 8px 8px; border: 1px solid #c9d2df; text-align: left; white-space: nowrap; }
  .ef-table td { padding: 4px 6px; border: 1px solid #dfe5ee; vertical-align: middle; height: 42px; }
  .ef-table tbody tr:nth-child(even) td { background: #fafbfd; }
  .ef-table .c { text-align: center; }
  .ef-table .ef-txt { min-width: 140px; }
  .ef-cell { width: 100%; min-height: 32px; padding: 4px 8px; border: 1px solid transparent; border-radius: 6px; background: transparent; font-size: 13px; }
  .ef-cell:hover { border-color: #dfe5ee; background: #fff; }
  .ef-signbtn { display: inline-flex; align-items: center; justify-content: center; gap: 4px; min-width: 100px; height: 32px; border: 1px dashed #9aa8bd; border-radius: 6px; background: #fff; color: #41506a; font-size: 12.5px; cursor: pointer; }
  .ef-signbtn.done { border-style: solid; border-color: #b9d8c2; background: #f2faf4; color: #1e6b34; }
  .ef-sig { height: 30px; max-width: 120px; object-fit: contain; }
  .ef-pass { color: #1e6b34; font-weight: 700; }
  .ef-fail { color: #c0392b; font-weight: 700; }
  .ef-ok { color: #1e6b34; }
  .ef-del { width: 28px; height: 28px; border: none; border-radius: 6px; background: transparent; color: #8a97ab; cursor: pointer; display: inline-flex; align-items: center; justify-content: center; }
  .ef-del:hover { background: #fdecea; color: #c0392b; }
  .ef-empty { text-align: center; color: #8a97ab; padding: 18px !important; height: auto !important; }
  .ef-foot { margin-top: 14px; padding-top: 8px; border-top: 1px solid #e6eaf0; font-size: 11.5px; color: #8a97ab; }

  .ef-side { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 12px; margin-top: 14px; max-width: 1100px; }
  .ef-card { background: #fff; border: 1px solid #dfe5ee; border-radius: 12px; padding: 12px 14px; }
  .ef-card h4 { display: flex; align-items: center; gap: 6px; margin: 0 0 8px; font-size: 13.5px; color: #0B3679; }
  .ef-files { margin: 0; padding-left: 18px; font-size: 13px; }
  .ef-steps { margin: 0; padding-left: 18px; font-size: 13px; display: flex; flex-direction: column; gap: 4px; }
  .ef-steps .s-return b, .ef-steps .s-cancel b { color: #c0392b; }
  .ef-steps .s-pass b { color: #1e6b34; }

  @media (max-width: 760px) {
    .ef-paper { padding: 14px; }
    .ef-head { grid-template-columns: 48px 1fr; }
    .ef-code { grid-column: 1 / -1; flex-direction: row; gap: 10px; align-items: center; }
    .ef-fields { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .ef-field { grid-column: 1 / -1 !important; }
  }
</style>
