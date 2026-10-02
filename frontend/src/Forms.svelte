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
    if (!ok) { err = body.error || 'ส่งไม่สำเร็จ'; return }
    openRec(body)
    msg = 'ส่งอนุมัติแล้ว ' + body.refNo
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
    setTimeout(() => w.print(), 400)
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

    {#if schema}
      <div class="grid gap-3 md:grid-cols-2">
        {#each schema.header_fields || [] as f}
          <div class="sgf">
            <label>{f.label}{#if f.required} <em>*</em>{/if}</label>
            {#if !editable()}
              <div>{f.type === 'date' ? thDate(data[f.key]) : f.type === 'checkbox' ? (data[f.key] || []).map((v) => optLabel(f, v)).join(', ') : f.type === 'radio' ? optLabel(f, data[f.key]) : (data[f.key] || '—')} {f.otherKey && data[f.otherKey] ? `(${data[f.otherKey]})` : ''}</div>
            {:else if f.type === 'date'}
              <input type="date" value={data[f.key] || ''} on:input={(e) => setField(f.key, e.currentTarget.value)} />
            {:else if f.type === 'time'}
              <input type="time" value={data[f.key] || ''} on:input={(e) => setField(f.key, e.currentTarget.value)} />
            {:else if f.type === 'select_or_text'}
              <input list="places-{f.key}" value={data[f.key] || ''} on:input={(e) => setField(f.key, e.currentTarget.value)} />
              <datalist id="places-{f.key}">{#each [...(f.options || []), ...places] as o}<option value={o}></option>{/each}</datalist>
            {:else if f.type === 'radio'}
              {#each f.options as o}
                <label class="inline-flex items-center gap-1 mr-3"><input type="radio" name={f.key} checked={data[f.key] === o.value} on:change={() => setField(f.key, o.value)} /> {o.label}</label>
              {/each}
              {#if data[f.key] === 'other'}<input placeholder="ระบุ" value={data[f.otherKey] || ''} on:input={(e) => setField(f.otherKey, e.currentTarget.value)} />{/if}
            {:else if f.type === 'checkbox'}
              {#each f.options as o}
                <label class="inline-flex items-center gap-1 mr-3"><input type="checkbox" checked={(data[f.key] || []).includes(o.value)} on:change={() => toggleCheck(f.key, o.value)} /> {o.label}</label>
              {/each}
              {#if (data[f.key] || []).includes('other')}<input placeholder="ระบุ" value={data[f.otherKey] || ''} on:input={(e) => setField(f.otherKey, e.currentTarget.value)} />{/if}
            {:else}
              <input value={data[f.key] || ''} on:input={(e) => setField(f.key, e.currentTarget.value)} />
            {/if}
          </div>
        {/each}
      </div>

      {#each schema.table_sections || [] as sec}
        <h3 class="mt-4">{sec.label}</h3>
        {#if editable()}
          <div class="flex flex-wrap gap-2 mb-2">
            <button class="mbtn gh" on:click={addRow}>เพิ่มแถว</button>
            {#if sec.copyPrevious}<button class="mbtn gh" on:click={copyPrev}>คัดลอกรายชื่อจากครั้งก่อน</button>{/if}
            {#if sec.deptFill}
              <select class="border border-line rounded-lg px-2" on:change={(e) => { if (e.currentTarget.value) addDept(e.currentTarget.value); e.currentTarget.value = '' }}>
                <option value="">เพิ่มทั้งแผนก</option>
                {#each DEPTS as d}<option value={d}>{DEPTNAME[d]}</option>{/each}
              </select>
            {/if}
          </div>
        {/if}
        <div class="overflow-x-auto">
          <table class="w-full text-[13px]">
            <thead><tr>
              <th>ลำดับ</th>
              {#each sec.columns as c}<th>{c.label}</th>{/each}
              {#if editable()}<th></th>{/if}
            </tr></thead>
            <tbody>
              {#each data.participants || [] as row, i}
                <tr>
                  <td>{i + 1}</td>
                  {#each sec.columns as c}
                    <td>
                      {#if !editable()}
                        {#if c.type === 'signature'}
                          {#if row.signature}<img alt="" src={row.signature} class="h-8" />{:else if row.attendedAt}เช็คอินแล้ว{:else}—{/if}
                        {:else if c.type === 'passfail'}
                          {row.result === 'pass' ? 'ผ่าน' : row.result === 'fail' ? 'ไม่ผ่าน' : '—'}
                        {:else}{row[c.key] || ''}{/if}
                      {:else if c.type === 'employee'}
                        <input class="w-24 border border-line rounded px-1" bind:value={row.empCode} on:change={() => onEmp(i)} />
                      {:else if c.type === 'passfail'}
                        <select bind:value={row.result} on:change={() => (dirty = true)}><option value="">—</option><option value="pass">ผ่าน</option><option value="fail">ไม่ผ่าน</option></select>
                      {:else if c.type === 'signature'}
                        <button class="mbtn gh" on:click={() => startSign(i)}>{row.signature ? 'ลงชื่อแล้ว' : 'เซ็น'}{row.attendedAt ? ' · เช็คอิน' : ''}</button>
                      {:else}
                        <input class="w-full border border-line rounded px-1" bind:value={row[c.key]} on:input={() => (dirty = true)} />
                      {/if}
                    </td>
                  {/each}
                  {#if editable()}<td><button class="mbtn gh" on:click={() => delRow(i)}>ลบ</button></td>{/if}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/each}
    {/if}

    <div class="mt-4">
      <b>ไฟล์แนบ</b>
      <ul>{#each rec.files || [] as f}<li><a href={`/api/forms/files/${f.id}`} on:click|preventDefault={async () => {
        const r = await fetch(`/api/forms/files/${f.id}`, { headers: { Authorization: 'Bearer ' + (localStorage.getItem('isoToken') || '') } })
        const b = await r.blob(); const u = URL.createObjectURL(b); const a = document.createElement('a'); a.href = u; a.download = f.filename; a.click()
      }}>{f.filename}</a></li>{/each}</ul>
      {#if editable()}<button class="mbtn gh" on:click={() => fileInput.click()}>แนบไฟล์</button><input bind:this={fileInput} type="file" class="hidden" on:change={onFile} />{/if}
    </div>

    {#if rec.checkinUrl && editable() || (rec.status === 'review' || rec.status === 'approve' || rec.status === 'approved')}
      <div class="mt-3">
        <b>QR เช็คอิน</b>
        <div class="w-36">{@html rec.checkinQr || ''}</div>
        <div class="note break-all">{rec.checkinUrl}</div>
      </div>
    {/if}

    <h3 class="mt-4">ขั้นตอน</h3>
    <ul class="text-[13px]">
      {#each rec.steps || [] as s}
        <li>{s.at?.slice(0, 16)} · {s.actorName} · {s.action} {s.comment ? `— ${s.comment}` : ''}</li>
      {/each}
    </ul>
    {#if (rec.history || []).length > 1}
      <h3>ฉบับทั้งหมด</h3>
      {#each rec.history as h}<button class="mbtn gh" on:click={() => openId(h.id)}>{h.refNo} ครั้งที่ {h.revNo} · {STATUS[h.status]}</button>{/each}
    {/if}

    <div class="sgact mt-3">
      {#if editable()}
        <button class="mbtn gh" on:click={() => save(false)}>บันทึกร่าง</button>
        <button class="mbtn accent" on:click={submitRec}>ส่งอนุมัติ</button>
      {/if}
      {#if (rec.status === 'review' && me.role !== 'preparer' && me.role !== 'viewer' && (isAdmin() || rec.preparedBy !== me.empCode)) || (rec.status === 'approve' && (me.role === 'approver' || isAdmin()) && (isAdmin() || rec.preparedBy !== me.empCode))}
        <input placeholder="ความเห็น / เหตุผลตีกลับ" bind:value={reason} />
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
</style>
