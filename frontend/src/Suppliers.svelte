<script>
  // ผู้ให้บริการภายนอก / ผู้ขาย (ISO 9001:2015 ข้อ 8.4)
  //   ทะเบียนผู้ขายที่อนุมัติ SUP-xxx → ประเมินตามรอบ (คุณภาพ 40 · ส่งมอบ 30 · ราคา 15 · บริการ 15) → เกรด A–D → สถานะ + กำหนดประเมินซ้ำ
  //   ผลประเมินต่ำ (เกรด C/D หรือมีหัวข้อได้ 1) → เปิด CAR แหล่งที่มา "ผู้ขาย"
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'
  export let me = null
  export let focus = '' // รหัสผู้ขายที่ต้องการเปิด (จากงานของฉัน/เตรียม Audit)
  const dispatch = createEventDispatcher()

  const STATUS = { approved: 'อนุมัติ', conditional: 'มีเงื่อนไข', blocked: 'ระงับ' }
  const STATUS_CLS = { approved: 's-closed', conditional: 's-in_progress', blocked: 's-overdue' }
  const CATEGORY = { material: 'วัตถุดิบ / ชิ้นส่วน', outsource: 'จ้างผลิต / จ้างช่วงกระบวนการ', service: 'บริการ (ขนส่ง สอบเทียบ ซ่อม…)', other: 'อื่น ๆ' }
  const CRITERIA = [
    ['quality', 'คุณภาพ', 40, 'ของเสีย/การตีกลับ ตรงสเปก ใบรับรอง'],
    ['delivery', 'การส่งมอบ', 30, 'ตรงเวลา ครบจำนวน'],
    ['price', 'ราคา', 15, 'เหมาะสม แข่งขันได้ คงที่'],
    ['service', 'บริการ', 15, 'การตอบสนอง แก้ปัญหา เอกสาร'],
  ]
  const SCORE_TEXT = ['', 'แย่มาก', 'ต้องปรับปรุง', 'พอใช้', 'ดี', 'ดีมาก']
  const EVENT = { create: 'ขึ้นทะเบียน', edit: 'แก้ไข', evaluate: 'ประเมิน', status: 'เปลี่ยนสถานะ', car: 'เปิด CAR' }
  // ต้องตรงกับ api/src/suppliers.rs (score/grade/recommended)
  const calcScore = (x) => Math.round(CRITERIA.reduce((a, [k, , w]) => a + Number(x[k]) * w, 0) / 5)
  const gradeOf = (sc) => (sc >= 85 ? 'A' : sc >= 70 ? 'B' : sc >= 50 ? 'C' : 'D')
  const minOf = (x) => Math.min(...CRITERIA.map(([k]) => Number(x[k])))
  const recommend = (g, mn) => (g === 'D' ? 'blocked' : g === 'C' || mn <= 1 ? 'conditional' : 'approved')
  const isPoor = (g, mn) => g === 'C' || g === 'D' || mn <= 1
  const today = () => new Date(Date.now() + 7 * 3600e3).toISOString().slice(0, 10)
  function addMonths(d, n) {
    if (!d || !n) return ''
    const [y, m, dd] = d.split('-').map(Number)
    const last = new Date(Date.UTC(y, m - 1 + Number(n) + 1, 0)).getUTCDate()
    return new Date(Date.UTC(y, m - 1 + Number(n), Math.min(dd, last))).toISOString().slice(0, 10)
  }
  const dueText = (p) => p.daysLeft == null ? '' : p.daysLeft < 0 ? `เลย ${-p.daysLeft} วัน` : p.daysLeft === 0 ? 'วันนี้' : `อีก ${p.daysLeft} วัน`
  $: approver = ['approver', 'admin'].includes(me?.role)

  let list = [], loading = true, err = '', q = ''
  let filter = 'active'
  async function load() {
    loading = true; err = ''
    try {
      const r = await api.suppliers()
      if (r.error) throw new Error(r.error)
      list = r
      if (focus) { const x = list.find((y) => y.code === focus); if (x) openDetail(x); focus = '' }
    } catch (e) { err = 'โหลดทะเบียนไม่สำเร็จ: ' + e.message }
    loading = false
  }
  onMount(load)

  const due = (p) => p.status !== 'blocked' && ['soon', 'overdue'].includes(p.dueState)
  const FILTERS = [
    ['active', 'ใช้งาน (อนุมัติ + มีเงื่อนไข)', (p) => p.status !== 'blocked'],
    ['approved', 'อนุมัติ', (p) => p.status === 'approved'],
    ['conditional', 'มีเงื่อนไข', (p) => p.status === 'conditional'],
    ['blocked', 'ระงับ', (p) => p.status === 'blocked'],
    ['due', 'ถึงกำหนดประเมิน', due],
    ['poor', 'ผลต่ำ ยังไม่มี CAR', (p) => p.poorNoCar > 0],
    ['mine', 'ที่ฉันรับผิดชอบ', (p) => p.ownerCode === me?.empCode],
    ['all', 'ทั้งหมด', () => true],
  ]
  $: counts = Object.fromEntries(FILTERS.map(([k, , fn]) => [k, list.filter(fn).length]))
  $: needle = q.trim().toLowerCase()
  $: shown = list.filter(FILTERS.find(([k]) => k === filter)[2])
    .filter((p) => !needle || [p.code, p.name, p.vendorCode, p.products, p.contact, p.ownerName].some((x) => (x || '').toLowerCase().includes(needle)))

  // ---- เลือกพนักงาน ----
  let people = [], pq = ''
  async function findPeople() {
    if (!pq.trim()) { people = []; return }
    try { people = (await api.employees(pq.trim())).slice(0, 8) } catch (e) { people = [] }
  }
  function pickPerson(p) { f.ownerCode = p.empCode; f.ownerName = p.nameTh; pq = p.nameTh; people = [] }

  // ---- ขึ้นทะเบียน/แก้ไข ----
  const KEYS = ['name', 'vendorCode', 'category', 'products', 'contact', 'phone', 'email', 'ownerCode', 'evalMonths', 'nextEval']
  const blank = () => ({ name: '', vendorCode: '', category: 'material', products: '', contact: '', phone: '', email: '', ownerCode: '', ownerName: '', evalMonths: 12, nextEval: '', status: 'conditional', reason: '' })
  let f = null, editing = null, saving = false, formErr = ''
  function openCreate() { f = blank(); editing = null; formErr = ''; pq = ''; people = [] }
  function openEdit() { f = { ...d }; editing = d; formErr = ''; pq = d.ownerName || ''; people = [] }
  async function saveForm() {
    if (!f.name.trim()) { formErr = 'กรุณาระบุชื่อผู้ขาย'; return }
    saving = true; formErr = ''
    const fields = Object.fromEntries(KEYS.map((k) => [k, k === 'evalMonths' ? Number(f[k]) : f[k]]))
    const r = editing ? await api.supplierUpdate(editing.id, editing.version, fields) : await api.supplierCreate({ ...fields, status: f.status, reason: f.reason })
    saving = false
    if (!r.ok) { formErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    f = null
    d = r.body.supplier
    msg = editing ? 'บันทึกการแก้ไขแล้ว' : `ขึ้นทะเบียน ${d.code} แล้ว`
    load()
  }

  // ---- รายละเอียด ----
  let d = null, msg = '', busy = false
  async function openDetail(x) {
    msg = ''; ev = null
    const r = await api.supplier(x.id)
    if (r.error) { err = r.error; return }
    d = r.supplier
  }

  // ---- ประเมิน ----
  let ev = null, evErr = ''
  function startEval() {
    const y = Number(today().slice(0, 4)) + 543
    ev = { period: `ปี ${y}`, evalDate: today(), quality: 4, delivery: 4, price: 4, service: 4, status: '', nextEval: '', note: '' }
    evErr = ''
  }
  $: evScore = ev ? calcScore(ev) : 0
  $: evGrade = gradeOf(evScore)
  $: evMin = ev ? minOf(ev) : 5
  $: evRec = recommend(evGrade, evMin)
  $: evStatus = ev?.status || evRec
  $: evNext = ev ? (evStatus === 'blocked' ? '' : ev.nextEval || addMonths(ev.evalDate, evStatus === 'approved' ? d?.evalMonths : 3)) : ''
  async function saveEval() {
    if (!ev.period.trim()) { evErr = 'กรุณาระบุรอบที่ประเมิน'; return }
    if ((isPoor(evGrade, evMin) || evStatus !== evRec) && !ev.note.trim()) { evErr = 'กรุณาบันทึกข้อมูลประกอบ/ปัญหาที่พบ/เหตุผลในหมายเหตุ'; return }
    busy = true; evErr = ''
    const r = await api.supplierEvaluate(d.id, { version: d.version, period: ev.period, evalDate: ev.evalDate, quality: Number(ev.quality), delivery: Number(ev.delivery),
      price: Number(ev.price), service: Number(ev.service), status: ev.status, nextEval: ev.nextEval, note: ev.note })
    busy = false
    if (!r.ok) { evErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    d = r.body.supplier; ev = null
    const v = d.evaluations[0]
    msg = `บันทึก ${v?.refNo || 'ผลประเมิน'} แล้ว · เกรด ${v?.grade} (${v?.score}%)` + (v?.poor ? ' — ผลต่ำ ควรเปิด CAR' : '')
    load()
  }
  async function raiseCar(v) {
    if (!confirm(`เปิด CAR จากผลประเมิน ${v.refNo} (เกรด ${v.grade})? — แหล่งที่มา "ผู้ขาย" · ผู้รับผิดชอบประเมินเป็นผู้รับผิดชอบใบ`)) return
    busy = true
    const r = await api.supplierCar(d.id, v.id)
    busy = false
    if (!r.ok) { alert(r.body.error || 'เปิด CAR ไม่สำเร็จ'); return }
    d = r.body.supplier; msg = `เปิด ${d.evaluations.find((x) => x.id === v.id)?.carRef || 'CAR'} แล้ว`; load()
  }
  async function setStatus(status) {
    const reason = prompt(`เหตุผลการเปลี่ยนสถานะเป็น "${STATUS[status]}" (เช่น ผลตรวจสถานที่ ปัญหาคุณภาพร้ายแรง)`)
    if (!reason || !reason.trim()) return
    const r = await api.supplierStatus(d.id, d.version, status, reason)
    if (!r.ok) { alert(r.body.error || 'ไม่สำเร็จ'); return }
    d = r.body.supplier; msg = `เปลี่ยนสถานะเป็น ${STATUS[status]} แล้ว`; load()
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="truck" size={20} /> ผู้ขายและการประเมิน</h2>
  <p class="cat-desc">ISO 9001:2015 ข้อ 8.4 — ทะเบียนผู้ให้บริการภายนอกที่อนุมัติ · เกณฑ์ประเมิน คุณภาพ 40% · การส่งมอบ 30% · ราคา 15% · บริการ 15% → เกรด A ≥ 85 · B 70–84 · C 50–69 · D &lt; 50 · ประเมินซ้ำตามรอบ</p>
</div>

<div class="stat-row">
  <div class="card accent"><div class="n">{list.filter((p) => p.status === 'approved').length}</div><div class="l">อนุมัติ</div></div>
  <div class="card"><div class="n {list.some((p) => p.status === 'conditional') ? '!text-warn' : ''}">{list.filter((p) => p.status === 'conditional').length}</div><div class="l">มีเงื่อนไข</div></div>
  <div class="card"><div class="n">{list.filter((p) => p.status === 'blocked').length}</div><div class="l">ระงับ</div></div>
  <div class="card"><div class="n {list.some((p) => p.evalOverdue) ? '!text-danger' : ''}">{list.filter(due).length}</div><div class="l">ถึงกำหนดประเมิน (เลย {list.filter((p) => p.evalOverdue).length})</div></div>
  <div class="card"><div class="n {list.some((p) => p.poorNoCar) ? '!text-danger' : ''}">{list.filter((p) => p.poorNoCar > 0).length}</div><div class="l">ผลต่ำ ยังไม่มี CAR</div></div>
</div>

<div class="type-tabs">
  <div class="relative flex-1 max-w-[420px]"><span class="search-ic"><Icon name="search" size={16} /></span><input class="cat-search !mb-0 pl-[38px]" placeholder="ค้นหารหัส ชื่อ สินค้า/บริการ ผู้ติดต่อ…" bind:value={q} /></div>
  <span class="flex-1"></span>
  {#if me && me.role !== 'viewer'}
    <button class="mbtn accent inline-flex items-center gap-1.5" on:click={openCreate}><Icon name="plus" size={15} /> ขึ้นทะเบียนผู้ขาย</button>
  {/if}
</div>
<div class="dept-chips">
  {#each FILTERS as [k, label]}<button class="dept-chip {filter === k ? 'active' : ''}" on:click={() => (filter = k)}>{label} <span class="dc-n">{counts[k]}</span></button>{/each}
</div>

{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}

{#if loading}
  <div class="loadwrap"><span class="spin"></span><p class="note m-0">กำลังโหลด…</p></div>
{:else if !shown.length}
  <div class="empty-state"><Icon name="truck" size={38} stroke={1.5} /><p>{list.length ? 'ไม่มีรายการในหมวดนี้' : 'ยังไม่มีทะเบียนผู้ขาย — เริ่มจากผู้ขายวัตถุดิบ/ชิ้นส่วน และผู้รับจ้างช่วงที่มีผลต่อคุณภาพสินค้า'}</p></div>
{:else}
  <div class="overflow-x-auto">
    <table class="chg-table">
      <thead><tr><th>รหัส</th><th>ผู้ขาย</th><th>สินค้า / บริการ</th><th>ผู้รับผิดชอบ</th><th>ผลประเมินล่าสุด</th><th>ประเมินครั้งถัดไป</th><th>สถานะ</th></tr></thead>
      <tbody>
        {#each shown as p (p.id)}
          <tr class="cursor-pointer hover:bg-brand-light/40" on:click={() => openDetail(p)}>
            <td class="nowrap font-semibold">{p.code}</td>
            <td><div class="font-semibold">{p.name}</div><div class="mut text-[12px]">{CATEGORY[p.category]}{p.vendorCode ? ' · ' + p.vendorCode : ''}</div></td>
            <td class="text-[12.5px]">{p.products || '—'}</td>
            <td>{p.ownerName || '—'}</td>
            <td class="nowrap">{#if p.lastGrade}<span class="grade-badge g-{p.lastGrade}">{p.lastGrade}</span> {p.lastScore}%<div class="mut text-[11.5px]">{p.lastEval}</div>{:else}<span class="mut">ยังไม่ประเมิน</span>{/if}
              {#if p.poorNoCar}<div class="text-danger text-[11.5px] font-bold">ผลต่ำ ยังไม่มี CAR</div>{/if}</td>
            <td class="nowrap {p.evalOverdue ? 'text-danger font-bold' : p.dueState === 'soon' ? 'text-warn font-bold' : ''}">{p.nextEval || '—'}{#if p.nextEval}<div class="text-[11.5px] {p.evalOverdue ? '' : 'mut'}">{dueText(p)}</div>{/if}</td>
            <td class="nowrap"><span class="status-badge {STATUS_CLS[p.status]}" title={p.statusReason}>{STATUS[p.status]}</span></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

{#if d && !f}
  <div class="mbg" on:mousedown|self={() => (d = null)}>
    <div class="mbox wide">
      <header class="mhead">
        <div class="mt"><b>{d.code}</b> {d.name}
          <div class="msub">{CATEGORY[d.category]} · ขึ้นทะเบียนโดย {d.createdName} · {fmtWhen(d.createdAt)}</div>
        </div>
        <button class="mx inline-flex items-center justify-center" on:click={() => (d = null)}><Icon name="x" size={16} /></button>
      </header>
      <div class="mbody">
        {#if msg}<div class="save-ok">{msg}</div>{/if}
        {#if d.evalOverdue}<div class="merr">เลยกำหนดประเมิน {-d.daysLeft} วัน</div>{/if}
        {#if d.poorNoCar}<div class="merr">มีผลประเมินต่ำที่ยังไม่เปิด CAR</div>{/if}
        {#if d.gaps.length}<div class="merr">ข้อมูลยังไม่ครบ: {d.gaps.join(', ')}</div>{/if}
        <div class="mr-bar">
          {#if d.can.evaluate}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={startEval}><Icon name="list-checks" size={14} /> ประเมินผู้ขาย</button>{/if}
          {#if d.can.edit}<button class="mbtn gh" on:click={openEdit}>แก้ไข</button>{/if}
          {#if d.can.setStatus}
            {#each Object.keys(STATUS).filter((k) => k !== d.status) as k}<button class="mbtn gh" on:click={() => setStatus(k)}>{k === 'blocked' ? 'ระงับการซื้อ' : 'เปลี่ยนเป็น' + STATUS[k]}</button>{/each}
          {/if}
          <span class="mnote">สถานะ <span class="status-badge {STATUS_CLS[d.status]}">{STATUS[d.status]}</span> {d.statusReason} · ประเมินครั้งถัดไป <b class={d.evalOverdue ? 'text-danger' : ''}>{d.nextEval || '—'}</b></span>
        </div>

        {#if ev}
          <section class="sec">
            <h2 class="!mb-2">ประเมินผู้ขาย (8.4.1)</h2>
            <div class="grid grid-cols-2 gap-3">
              <div class="sgf"><label>รอบ / ช่วงที่ประเมิน <em>*</em></label><input bind:value={ev.period} placeholder="เช่น ม.ค.–มิ.ย. 2569" /></div>
              <div class="sgf"><label>วันที่ประเมิน</label><input type="date" max={today()} bind:value={ev.evalDate} /></div>
            </div>
            <div class="sev-grid">
              {#each CRITERIA as [k, label, w, hint]}
                <div class="sev-row">
                  <div><b>{label}</b> <span class="mut text-[11.5px]">{w}% · {hint}</span></div>
                  <div class="seg">{#each [1, 2, 3, 4, 5] as n}<button class={Number(ev[k]) === n ? 'on sc-' + n : ''} title={SCORE_TEXT[n]} on:click={() => (ev[k] = n)}>{n}</button>{/each}</div>
                  <span class="mut text-[12px] w-[84px]">{SCORE_TEXT[ev[k]]}</span>
                </div>
              {/each}
            </div>
            <div class="sev-total">
              คะแนนรวม <b>{evScore}%</b> <span class="grade-badge g-{evGrade}">{evGrade}</span>
              · สถานะที่แนะนำ <span class="status-badge {STATUS_CLS[evRec]}">{STATUS[evRec]}</span>
              {#if isPoor(evGrade, evMin)}<span class="text-danger font-bold"> · ผลต่ำ — เปิด CAR ได้หลังบันทึก</span>{/if}
            </div>
            <div class="grid grid-cols-2 gap-3">
              <div class="sgf"><label>สถานะหลังประเมิน</label>
                {#if approver}<select bind:value={ev.status}><option value="">ตามที่แนะนำ ({STATUS[evRec]})</option>{#each Object.entries(STATUS) as [k, l]}<option value={k}>{l}</option>{/each}</select>
                {:else}<div class="pt-1.5"><span class="status-badge {STATUS_CLS[evRec]}">{STATUS[evRec]}</span> <span class="mut text-[11.5px]">ตามผลประเมิน (ผู้ตรวจสอบขึ้นไปกำหนดต่างได้)</span></div>{/if}
              </div>
              <div class="sgf"><label>ประเมินครั้งถัดไป</label>
                {#if evStatus === 'blocked'}<div class="pt-1.5 mut text-[12.5px]">ระงับ — ไม่มีกำหนดประเมิน</div>
                {:else}<input type="date" min={ev.evalDate} bind:value={ev.nextEval} /><div class="mut text-[11.5px]">ว่าง = {evNext} ({evStatus === 'approved' ? `รอบ ${d.evalMonths} เดือน` : 'มีเงื่อนไข 3 เดือน'})</div>{/if}
              </div>
            </div>
            <div class="sgf"><label>ข้อมูลประกอบ / ปัญหาที่พบ{isPoor(evGrade, evMin) || evStatus !== evRec ? ' *' : ''}</label><textarea rows="2" bind:value={ev.note} placeholder="เช่น ส่งตรงเวลา 92% (46/50) · ตีกลับ 2 ล็อต · ราคาปรับขึ้น 5%"></textarea></div>
            {#if evErr}<div class="merr">{evErr}</div>{/if}
            <div class="sgact !justify-start"><button class="mbtn accent" disabled={busy} on:click={saveEval}>{busy ? 'กำลังบันทึก…' : 'บันทึกผลประเมิน'}</button><button class="mbtn gh" on:click={() => (ev = null)}>ยกเลิก</button></div>
          </section>
        {/if}

        <section class="sec">
          <h2 class="!mb-2">ข้อมูลผู้ขาย</h2>
          <div class="det-info">
            <div><b>รหัสในระบบบัญชี:</b> {d.vendorCode || '—'}</div><div><b>ประเภท:</b> {CATEGORY[d.category]}</div>
            <div class="col-span-2"><b>สินค้า/บริการที่ซื้อ:</b> {d.products || '—'}</div>
            <div><b>ผู้ติดต่อ:</b> {d.contact || '—'}</div><div><b>โทร / อีเมล:</b> {[d.phone, d.email].filter(Boolean).join(' · ') || '—'}</div>
            <div><b>ผู้รับผิดชอบประเมิน:</b> {d.ownerName || '—'}</div><div><b>รอบประเมิน:</b> ทุก {d.evalMonths} เดือน (มีเงื่อนไข 3 เดือน)</div>
          </div>
        </section>

        <h3 class="rd-sub">ผลการประเมิน</h3>
        {#if d.evaluations.length}
          <div class="overflow-x-auto">
            <table class="chg-table">
              <thead><tr><th>เลขที่</th><th>รอบ</th>{#each CRITERIA as [, label]}<th class="text-center">{label}</th>{/each}<th>รวม</th><th>สถานะ</th><th>หมายเหตุ</th><th>CAR</th></tr></thead>
              <tbody>
                {#each d.evaluations as v (v.id)}
                  <tr>
                    <td class="nowrap font-semibold">{v.refNo}<div class="mut text-[11.5px] font-normal">{v.evalDate} · {v.evaluatorName}</div></td>
                    <td>{v.period}</td>
                    {#each CRITERIA as [k]}<td class="text-center"><span class="sc-dot sc-{v[k]}">{v[k]}</span></td>{/each}
                    <td class="nowrap"><span class="grade-badge g-{v.grade}">{v.grade}</span> {v.score}%</td>
                    <td class="nowrap text-[12px]">{#if v.statusBefore !== v.statusAfter}{STATUS[v.statusBefore]} → {/if}<span class="status-badge {STATUS_CLS[v.statusAfter]}">{STATUS[v.statusAfter]}</span>{#if v.nextEval}<div class="mut">ถัดไป {v.nextEval}</div>{/if}</td>
                    <td class="text-[12.5px] whitespace-pre-wrap">{v.note || '—'}</td>
                    <td class="nowrap">
                      {#if v.carRef}<button class="mbtn gh !py-0.5" on:click={() => dispatch('goto', { tab: 'act' })}>{v.carRef}</button>
                      {:else if v.can?.car}<button class="mbtn accent !py-0.5" disabled={busy} on:click={() => raiseCar(v)}>เปิด CAR</button>
                      {:else if v.poor}<span class="text-danger text-[12px]">ผลต่ำ</span>{:else}—{/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}<p class="mnote">ยังไม่มีผลประเมิน{d.status === 'conditional' ? ' — ผู้ขายใหม่ต้องประเมินก่อนอนุมัติ' : ''}</p>{/if}

        {#if d.history?.length}
          <h3 class="rd-sub">ประวัติ</h3>
          <table class="chg-table"><thead><tr><th>เมื่อ</th><th>ผู้ดำเนินการ</th><th>การดำเนินการ</th><th>รายละเอียด</th></tr></thead>
            <tbody>{#each [...d.history].reverse() as h}<tr><td class="nowrap">{fmtWhen(h.at)}</td><td>{h.actorName}</td><td>{EVENT[h.event] || h.event}</td><td>{h.note || '—'}</td></tr>{/each}</tbody></table>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if f}
  <div class="sgbg" on:mousedown|self={() => (f = null)}>
    <div class="sgbox wide">
      <h3>{editing ? `แก้ไข ${editing.code}` : 'ขึ้นทะเบียนผู้ขาย'}</h3>
      <div class="grid grid-cols-[2fr_1fr] gap-3">
        <div class="sgf"><label>ชื่อผู้ขาย / บริษัท <em>*</em></label><input bind:value={f.name} /></div>
        <div class="sgf"><label>รหัสในระบบบัญชี</label><input bind:value={f.vendorCode} /></div>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ประเภท</label><select bind:value={f.category}>{#each Object.entries(CATEGORY) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
        <div class="sgf"><label>รอบประเมิน (เดือน)</label><input type="number" min="1" max="36" bind:value={f.evalMonths} /></div>
      </div>
      <div class="sgf"><label>สินค้า / บริการที่ซื้อ <em>*</em></label><input bind:value={f.products} placeholder="เช่น ลวดเหล็กชุบสังกะสี, เสารั้ว, บริการขนส่ง" /></div>
      <div class="grid grid-cols-3 gap-3">
        <div class="sgf"><label>ผู้ติดต่อ</label><input bind:value={f.contact} /></div>
        <div class="sgf"><label>โทรศัพท์</label><input bind:value={f.phone} /></div>
        <div class="sgf"><label>อีเมล</label><input bind:value={f.email} /></div>
      </div>
      <div class="sgf mr-person"><label>ผู้รับผิดชอบประเมิน <em>*</em></label>
        <input bind:value={pq} on:input={findPeople} placeholder="พิมพ์ชื่อหรือรหัสพนักงาน (มักเป็นฝ่ายจัดซื้อ)" />
        {#if f.ownerCode}<div class="mut text-[12px]">เลือก: {f.ownerName} ({f.ownerCode})</div>{/if}
        {#if people.length}<div class="mr-people">{#each people as p}<button on:click={() => pickPerson(p)}>{p.empCode} · {p.nameTh} <span class="mut">{p.position}</span></button>{/each}</div>{/if}
      </div>
      {#if editing}
        {#if editing.status !== 'blocked'}<div class="sgf"><label>กำหนดประเมินครั้งถัดไป</label><input type="date" bind:value={f.nextEval} /></div>{/if}
      {:else}
        <div class="grid grid-cols-2 gap-3">
          <div class="sgf"><label>สถานะเริ่มต้น</label>
            {#if approver}<select bind:value={f.status}>{#each Object.entries(STATUS) as [k, l]}<option value={k}>{l}{k === 'conditional' ? ' (ผู้ขายใหม่ — รอประเมิน)' : k === 'approved' ? ' (ผู้ขายเดิมที่อนุมัติแล้ว)' : ''}</option>{/each}</select>
            {:else}<div class="pt-1.5"><span class="status-badge s-in_progress">มีเงื่อนไข</span> <span class="mut text-[11.5px]">จนกว่าจะประเมินครั้งแรก</span></div>{/if}
          </div>
          {#if f.status !== 'blocked'}<div class="sgf"><label>กำหนดประเมิน</label><input type="date" min={today()} bind:value={f.nextEval} /><div class="mut text-[11.5px]">ว่าง = {f.status === 'approved' ? `อีก ${f.evalMonths} เดือน` : 'ภายใน 30 วัน'}</div></div>{/if}
        </div>
        {#if f.status !== 'conditional'}<div class="sgf"><label>เหตุผล / หลักฐานการ{STATUS[f.status]} <em>*</em></label><input bind:value={f.reason} placeholder="เช่น อยู่ในรายชื่อผู้ขายที่อนุมัติ (FM-PU-xxx) ก่อนใช้ระบบ" /></div>{/if}
      {/if}
      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact"><button class="mbtn gh" on:click={() => (f = null)}>ยกเลิก</button><button class="mbtn accent" disabled={saving} on:click={saveForm}>{saving ? 'กำลังบันทึก…' : 'บันทึก'}</button></div>
    </div>
  </div>
{/if}
