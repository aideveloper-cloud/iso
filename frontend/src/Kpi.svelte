<script>
  // วัตถุประสงค์คุณภาพและตัวชี้วัด (ISO 9001:2015 ข้อ 6.2, 9.1)
  //   ตั้งเป้า (ผู้ตรวจสอบขึ้นไป) → ผู้รับผิดชอบบันทึกผลรายงวด → ไม่ถึงเป้าเปิด CAR → ป้อนการทบทวนฝ่ายบริหารอัตโนมัติ
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import { fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'
  export let me = null
  export let focus = '' // รหัส KPI ที่ต้องการเปิด (จากงานของฉัน)
  const dispatch = createEventDispatcher()

  const FREQ = { monthly: 'รายเดือน', quarterly: 'รายไตรมาส', yearly: 'รายปี' }
  const MONTHS = ['', 'ม.ค.', 'ก.พ.', 'มี.ค.', 'เม.ย.', 'พ.ค.', 'มิ.ย.', 'ก.ค.', 'ส.ค.', 'ก.ย.', 'ต.ค.', 'พ.ย.', 'ธ.ค.']

  /** แสดงงวดเป็น พ.ศ.: 2026-03 → มี.ค. 2569 · 2026-Q1 → ไตรมาส 1/2569 · 2026 → ปี 2569 */
  function periodLabel(p) {
    if (!p) return ''
    const q = /^(\d{4})-Q([1-4])$/.exec(p)
    if (q) return `ไตรมาส ${q[2]}/${+q[1] + 543}`
    const m = /^(\d{4})-(\d{2})$/.exec(p)
    if (m) return `${MONTHS[+m[2]]} ${+m[1] + 543}`
    return /^\d{4}$/.test(p) ? `ปี ${+p + 543}` : p
  }
  /** งวดย้อนหลัง n งวดนับจากงวดปัจจุบัน (ใหม่ → เก่า) */
  function recentPeriods(freq, n = 12) {
    const t = new Date(Date.now() + 7 * 3600e3)
    let y = t.getUTCFullYear(), mo = t.getUTCMonth() + 1
    const out = []
    if (freq === 'yearly') { for (let i = 0; i < n; i++) out.push(String(y - i)); return out }
    if (freq === 'quarterly') {
      let q = Math.floor((mo - 1) / 3) + 1
      for (let i = 0; i < n; i++) { out.push(`${y}-Q${q}`); q -= 1; if (q === 0) { q = 4; y -= 1 } }
      return out
    }
    for (let i = 0; i < n; i++) { out.push(`${y}-${String(mo).padStart(2, '0')}`); mo -= 1; if (mo === 0) { mo = 12; y -= 1 } }
    return out
  }
  const fmtNum = (v) => (v === null || v === undefined || v === '' ? '—' : Number(v).toLocaleString('th-TH', { maximumFractionDigits: 2 }))
  const cmp = (o) => (o.direction === 'lte' ? '≤' : '≥')

  let objectives = [], canCreate = false, graceDays = 15, loading = true, err = ''
  let filter = 'active', dept = ''
  async function load() {
    loading = true; err = ''
    try {
      const r = await api.kpiList()
      if (r.error) throw new Error(r.error)
      objectives = r.objectives; canCreate = r.canCreate; graceDays = r.graceDays
      if (focus) { const o = objectives.find((x) => x.code === focus); if (o) openDetail(o); focus = '' }
      else if (d) { const o = objectives.find((x) => x.id === d.id); if (o) setDetail(o) }
    } catch (e) { err = 'โหลดข้อมูลไม่สำเร็จ: ' + e.message }
    loading = false
  }
  onMount(load)

  const FILTERS = [
    ['active', 'ใช้งาน', (o) => o.status === 'active'],
    ['mine', 'ที่ฉันรับผิดชอบ', (o) => o.status === 'active' && o.ownerCode === me?.empCode],
    ['missing', 'ค้างบันทึกผล', (o) => o.missing],
    ['off', 'ไม่ถึงเป้า (งวดล่าสุด)', (o) => o.status === 'active' && o.latest && !o.latest.met],
    ['retired', 'เลิกใช้', (o) => o.status === 'retired'],
  ]
  $: base = objectives.filter((o) => !dept || o.dept === dept)
  $: counts = Object.fromEntries(FILTERS.map(([k, , fn]) => [k, base.filter(fn).length]))
  $: shown = base.filter(FILTERS.find(([k]) => k === filter)[2])
  $: active = objectives.filter((o) => o.status === 'active')
  $: measured = active.filter((o) => o.latest)
  $: metNow = measured.filter((o) => o.latest.met).length
  $: deptsUsed = DEPTS.filter((dp) => objectives.some((o) => o.dept === dp))

  // ---- กราฟย่อ (12 งวดล่าสุด เทียบเส้นเป้า) ----
  function spark(o, w = 120, h = 32) {
    const rs = (o.results || []).slice(-12)
    if (!rs.length) return null
    const vals = rs.map((r) => r.actual).concat([o.target])
    const lo = Math.min(...vals), hi = Math.max(...vals)
    const span = hi - lo || 1
    const x = (i) => (rs.length === 1 ? w / 2 : 4 + (i * (w - 8)) / (rs.length - 1))
    const y = (v) => h - 4 - ((v - lo) / span) * (h - 8)
    return { w, h, ty: y(o.target), pts: rs.map((r, i) => ({ x: x(i), y: y(r.actual), met: r.met, label: `${periodLabel(r.period)}: ${fmtNum(r.actual)}${o.unit}` })), line: rs.map((r, i) => `${x(i)},${y(r.actual)}`).join(' ') }
  }

  // ---- เลือกพนักงาน ----
  let people = [], pq = ''
  async function findPeople() {
    if (!pq.trim()) { people = []; return }
    try { people = (await api.employees(pq.trim())).slice(0, 8) } catch (e) { people = [] }
  }
  function pickPerson(p) { f.ownerCode = p.empCode; f.ownerName = p.nameTh; pq = p.nameTh; people = [] }

  // ---- สร้าง/แก้ไข ----
  const KEYS = ['title', 'dept', 'process', 'ownerCode', 'unit', 'direction', 'target', 'frequency', 'startPeriod', 'formula', 'plan', 'resources', 'evaluation']
  const blank = () => ({ title: '', dept: '', process: '', ownerCode: '', ownerName: '', unit: '%', direction: 'gte', target: '', frequency: 'monthly', startPeriod: '', formula: '', plan: '', resources: '', evaluation: '' })
  let f = null, editing = null, saving = false, formErr = ''
  function openCreate() { f = blank(); editing = null; formErr = ''; pq = ''; people = [] }
  function openEdit() { f = { ...d }; editing = d; formErr = ''; pq = d.ownerName || ''; people = [] }
  async function saveForm() {
    if (!f.title.trim()) { formErr = 'กรุณาระบุวัตถุประสงค์/ตัวชี้วัด'; return }
    if (f.target === '' || isNaN(Number(f.target))) { formErr = 'กรุณาระบุเป้าหมายเป็นตัวเลข'; return }
    saving = true; formErr = ''
    const fields = Object.fromEntries(KEYS.map((k) => [k, k === 'target' ? Number(f[k]) : f[k]]))
    const r = editing ? await api.kpiUpdate(editing.id, editing.version, fields) : await api.kpiCreate(fields)
    saving = false
    if (!r.ok) { formErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    f = null
    setDetail(r.body.objective)
    msg = editing ? 'บันทึกการแก้ไขแล้ว' : `สร้าง ${r.body.objective.code} แล้ว`
    load()
  }

  // ---- รายละเอียด / บันทึกผล ----
  let d = null, msg = '', rec = null, recErr = '', busy = false
  function setDetail(o) { d = o }
  function openDetail(o) { d = o; msg = ''; rec = null }
  /** งวดเริ่มต้น: งวดที่ควรมีผลถ้ายังไม่บันทึก ไม่งั้นงวดปัจจุบัน — งวดที่มีผลแล้วเติมค่าเดิมให้แก้ */
  function startRecord(period) {
    const has = (p) => d.results.some((r) => r.period === p)
    const p = period || (d.expectedPeriod && !has(d.expectedPeriod) ? d.expectedPeriod : recentPeriods(d.frequency)[0])
    rec = { period: p, actual: '', note: '', evidence: '' }
    fillRecord()
    recErr = ''
  }
  function fillRecord() {
    const old = d.results.find((r) => r.period === rec.period)
    rec = { ...rec, actual: old ? old.actual : '', note: old?.note || '', evidence: old?.evidence || '' }
  }
  $: recOld = rec && d ? d.results.find((r) => r.period === rec.period) : null
  async function saveRecord() {
    if (rec.actual === '' || isNaN(Number(rec.actual))) { recErr = 'กรุณากรอกผลเป็นตัวเลข'; return }
    busy = true; recErr = ''
    const r = await api.kpiRecord(d.id, { period: rec.period, actual: Number(rec.actual), note: rec.note, evidence: rec.evidence })
    busy = false
    if (!r.ok) { recErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    const res = r.body.objective.results.find((x) => x.period === rec.period)
    msg = `บันทึกผล ${periodLabel(rec.period)} แล้ว — ${res?.met ? 'ถึงเป้า' : 'ไม่ถึงเป้า (เปิด CAR ได้จากตารางผล)'}`
    rec = null
    setDetail(r.body.objective)
    load()
  }
  async function raiseCar(period) {
    if (!confirm(`เปิด CAR สำหรับผล ${periodLabel(period)} ที่ไม่ถึงเป้า?`)) return
    busy = true
    const r = await api.kpiCar(d.id, period)
    busy = false
    if (!r.ok) { msg = ''; alert(r.body.error || 'เปิด CAR ไม่สำเร็จ'); return }
    const car = r.body.objective.results.find((x) => x.period === period)?.carRef
    msg = `เปิด ${car} แล้ว — ติดตามต่อที่เมนู CAR / DAR / PAR`
    setDetail(r.body.objective)
    load()
  }
  async function retire() {
    const reason = prompt('เหตุผลการเลิกใช้ตัวชี้วัดนี้ (ผลเดิมยังเก็บไว้)')
    if (!reason || !reason.trim()) return
    const r = await api.kpiRetire(d.id, d.version, reason)
    if (!r.ok) { alert(r.body.error || 'ไม่สำเร็จ'); return }
    setDetail(r.body.objective); msg = 'เลิกใช้แล้ว'; load()
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="target" size={20} /> วัตถุประสงค์คุณภาพ (KPI)</h2>
  <p class="cat-desc">ISO 9001:2015 ข้อ 6.2 และ 9.1 — เป้าหมายที่วัดได้ ผู้รับผิดชอบ แผนการบรรลุ ผลรายงวด และการแก้ไขเมื่อไม่ถึงเป้า · ผลถูกดึงเข้าการทบทวนฝ่ายบริหารอัตโนมัติ</p>
</div>

<div class="grid grid-cols-2 md:grid-cols-4 gap-3 mb-4">
  <div class="card"><div class="n">{active.length}</div><div class="l">ตัวชี้วัดที่ใช้งาน</div></div>
  <div class="card accent"><div class="n">{measured.length ? Math.round((metNow / measured.length) * 100) + '%' : '—'}</div><div class="l">ถึงเป้าในงวดล่าสุด ({metNow}/{measured.length})</div></div>
  <div class="card"><div class="n {counts.missing ? '!text-danger' : ''}">{objectives.filter((o) => o.missing).length}</div><div class="l">ค้างบันทึกผล (เกิน {graceDays} วันหลังสิ้นงวด)</div></div>
  <div class="card"><div class="n {active.some((o) => o.offTargetNoCar.length) ? '!text-danger' : ''}">{active.filter((o) => o.offTargetNoCar.length).length}</div><div class="l">ไม่ถึงเป้าแต่ยังไม่เปิด CAR</div></div>
</div>

<div class="type-tabs">
  {#each FILTERS as [k, label]}
    <button class="type-tab {filter === k ? 'active' : ''}" on:click={() => (filter = k)}>{label} <span class="tt-n">{counts[k]}</span></button>
  {/each}
  <span class="flex-1"></span>
  <select class="mbtn gh !py-1" bind:value={dept}><option value="">ทุกแผนก</option>{#each deptsUsed as dp}<option value={dp}>{DEPTNAME[dp]}</option>{/each}</select>
  {#if canCreate}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={openCreate}><Icon name="plus" size={15} /> เพิ่มวัตถุประสงค์</button>{/if}
</div>

{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}

{#if loading}
  <div class="loadwrap"><span class="spin"></span><p class="note m-0">กำลังโหลด…</p></div>
{:else if !shown.length}
  <div class="empty-state"><Icon name="target" size={38} stroke={1.5} />
    <p>{objectives.length ? 'ไม่มีตัวชี้วัดในหมวดนี้' : 'ยังไม่มีวัตถุประสงค์คุณภาพในระบบ'}</p>
    {#if !objectives.length && canCreate}<button class="mbtn accent" on:click={openCreate}>เพิ่มวัตถุประสงค์แรก</button>{/if}
    {#if !objectives.length && !canCreate}<p class="mut text-[12.5px]">ผู้ตรวจสอบ/ผู้อนุมัติ (QMR) เป็นผู้กำหนดวัตถุประสงค์คุณภาพ</p>{/if}
  </div>
{:else}
  <div class="overflow-x-auto">
    <table class="chg-table kpi-table">
      <thead><tr><th>รหัส</th><th>วัตถุประสงค์ / ตัวชี้วัด</th><th>ผู้รับผิดชอบ</th><th class="text-right">เป้าหมาย</th><th class="text-right">ผลล่าสุด</th><th>แนวโน้ม 12 งวด</th><th>สถานะ</th></tr></thead>
      <tbody>
        {#each shown as o (o.id)}
          {@const sp = spark(o)}
          <tr class="cursor-pointer hover:bg-brand-light/40" on:click={() => openDetail(o)}>
            <td class="nowrap font-semibold">{o.code}</td>
            <td><div class="font-semibold">{o.title}</div><div class="mut text-[12px]">{o.dept ? DEPTNAME[o.dept] || o.dept : ''}{o.process ? ' · ' + o.process : ''} · {FREQ[o.frequency]}</div></td>
            <td>{o.ownerName || '—'}</td>
            <td class="text-right nowrap">{cmp(o)} {fmtNum(o.target)} {o.unit}</td>
            <td class="text-right nowrap">
              {#if o.latest}
                <span class="font-bold {o.latest.met ? 'text-ok' : 'text-danger'}">{fmtNum(o.latest.actual)} {o.unit}</span>
                {#if o.trend === 'better'}<span class="text-ok" title="ดีขึ้นจากงวดก่อน"><Icon name="trending-up" size={14} /></span>{:else if o.trend === 'worse'}<span class="text-danger" title="แย่ลงจากงวดก่อน"><Icon name="trending-down" size={14} /></span>{/if}
                <div class="mut text-[11.5px]">{periodLabel(o.latest.period)}</div>
              {:else}<span class="mut">ยังไม่มีผล</span>{/if}
            </td>
            <td>
              {#if sp}
                <svg width={sp.w} height={sp.h} class="block">
                  <line x1="0" x2={sp.w} y1={sp.ty} y2={sp.ty} stroke="currentColor" class="text-mut" stroke-dasharray="3 3" stroke-width="1" />
                  <polyline points={sp.line} fill="none" stroke="currentColor" class="text-brand" stroke-width="1.5" />
                  {#each sp.pts as p}<circle cx={p.x} cy={p.y} r="2.5" class={p.met ? 'fill-ok' : 'fill-danger'}><title>{p.label}</title></circle>{/each}
                </svg>
              {:else}<span class="mut text-[12px]">—</span>{/if}
            </td>
            <td class="nowrap">
              {#if o.status === 'retired'}<span class="status-badge s-closed">เลิกใช้</span>
              {:else if o.missing}<span class="status-badge s-overdue">ค้างบันทึก {periodLabel(o.expectedPeriod)}</span>
              {:else if o.pending}<span class="status-badge s-in_progress">รอบันทึก {periodLabel(o.expectedPeriod)}</span>
              {:else if o.offTargetNoCar.length}<span class="status-badge s-overdue">ไม่ถึงเป้า · ยังไม่เปิด CAR</span>
              {:else}<span class="status-badge s-closed">เป็นปัจจุบัน</span>{/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  <p class="mnote mt-2">จุดสีเขียว = ถึงเป้า · สีแดง = ไม่ถึงเป้า · เส้นประ = เป้าหมาย · ผลต้องบันทึกภายใน {graceDays} วันหลังสิ้นงวด</p>
{/if}

{#if d && !f}
  <div class="mbg" on:mousedown|self={() => (d = null)}>
    <div class="mbox wide">
      <header class="mhead">
        <div class="mt"><b>{d.code}</b> {d.title}
          <div class="msub">{d.dept ? DEPTNAME[d.dept] || d.dept : 'ไม่ระบุแผนก'} · {FREQ[d.frequency]} · เป้าหมาย {cmp(d)} {fmtNum(d.target)} {d.unit} · ผู้รับผิดชอบ {d.ownerName || '—'}</div>
        </div>
        <button class="mx inline-flex items-center justify-center" on:click={() => (d = null)}><Icon name="x" size={16} /></button>
      </header>
      <div class="mbody">
        {#if msg}<div class="save-ok">{msg}</div>{/if}
        {#if d.status === 'retired'}<div class="merr">เลิกใช้แล้ว: {d.retireReason}</div>{/if}
        <div class="mr-bar">
          {#if d.can.record}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => startRecord('')}><Icon name="pen-line" size={14} /> บันทึกผล{d.expectedPeriod && (d.missing || d.pending) ? ' ' + periodLabel(d.expectedPeriod) : ''}</button>{/if}
          {#if d.can.edit}<button class="mbtn gh" on:click={openEdit}>แก้ไขวัตถุประสงค์</button>{/if}
          {#if d.can.retire}<button class="mbtn gh" on:click={retire}>เลิกใช้</button>{/if}
          <span class="mnote">อัตราบรรลุ {d.rate === null ? '—' : d.rate + '%'} ({d.achieved}/{d.measured} งวด)</span>
        </div>

        {#if rec}
          <section class="sec">
            <h2 class="!mb-2">บันทึกผลงวด</h2>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
              <div class="sgf"><label>งวด</label><select bind:value={rec.period} on:change={fillRecord}>{#each recentPeriods(d.frequency) as p}<option value={p}>{periodLabel(p)}{d.results.some((r) => r.period === p) ? ' (มีผลแล้ว)' : ''}</option>{/each}</select></div>
              <div class="sgf"><label>ผลที่วัดได้ ({d.unit || 'หน่วย'}) — เป้า {cmp(d)} {fmtNum(recOld ? recOld.target : d.target)}</label><input type="number" step="any" bind:value={rec.actual} /></div>
              <div class="sgf"><label>หลักฐาน (รหัสเอกสาร/เลขที่บันทึก/ลิงก์)</label><input bind:value={rec.evidence} placeholder="เช่น FM-QC-005 ต.ค. 69" /></div>
            </div>
            <div class="sgf"><label>หมายเหตุ / วิเคราะห์ผล</label><textarea rows="2" bind:value={rec.note} placeholder="สาเหตุที่ผลเปลี่ยนแปลง ปัจจัยที่เกี่ยวข้อง"></textarea></div>
            {#if recOld}<p class="mnote">งวดนี้บันทึกไว้แล้ว ({fmtNum(recOld.actual)} {d.unit} โดย {recOld.recordedName}) — บันทึกใหม่จะแทนค่าเดิม และค่าเดิมถูกเก็บในบันทึกการใช้งาน</p>{/if}
            {#if recErr}<div class="merr">{recErr}</div>{/if}
            <div class="sgact !justify-start"><button class="mbtn accent" disabled={busy} on:click={saveRecord}>{busy ? 'กำลังบันทึก…' : 'บันทึกผล'}</button><button class="mbtn gh" on:click={() => (rec = null)}>ยกเลิก</button></div>
          </section>
        {/if}

        <div class="capa-grid">
          <section class="sec">
            <h2 class="!mb-2">นิยามตัวชี้วัด</h2>
            <div class="det-info">
              <div><b>กระบวนการ:</b> {d.process || '—'}</div><div><b>งวดเริ่มวัด:</b> {d.startPeriod ? periodLabel(d.startPeriod) : '—'}</div>
              <div class="col-span-2"><b>วิธีคำนวณ:</b> {d.formula || '—'}</div>
            </div>
          </section>
          <section class="sec">
            <h2 class="!mb-2">แผนการบรรลุ (6.2.2)</h2>
            <div class="det-info">
              <div class="col-span-2"><b>สิ่งที่จะทำ / ใคร / เมื่อไร:</b> <span class="whitespace-pre-wrap">{d.plan || '—'}</span></div>
              <div class="col-span-2"><b>ทรัพยากร:</b> {d.resources || '—'}</div>
              <div class="col-span-2"><b>การประเมินผล:</b> {d.evaluation || '—'}</div>
            </div>
          </section>
        </div>

        <h3 class="rd-sub">ผลรายงวด</h3>
        {#if d.results.length}
          <table class="chg-table">
            <thead><tr><th>งวด</th><th class="text-right">ผล</th><th class="text-right">เป้า</th><th>ผล</th><th>หมายเหตุ / หลักฐาน</th><th>บันทึกโดย</th><th></th></tr></thead>
            <tbody>
              {#each [...d.results].reverse() as r}
                <tr>
                  <td class="nowrap">{periodLabel(r.period)}</td>
                  <td class="text-right nowrap font-bold {r.met ? 'text-ok' : 'text-danger'}">{fmtNum(r.actual)} {d.unit}</td>
                  <td class="text-right nowrap">{cmp(d)} {fmtNum(r.target)}</td>
                  <td>{#if r.met}<span class="status-badge s-closed">ถึงเป้า</span>{:else}<span class="status-badge s-overdue">ไม่ถึงเป้า</span>{/if}</td>
                  <td>{r.note || ''}{#if r.evidence}<div class="mut text-[12px]">หลักฐาน: {r.evidence}</div>{/if}</td>
                  <td class="nowrap">{r.recordedName}<div class="mut text-[11.5px]">{fmtWhen(r.recordedAt)}</div></td>
                  <td class="nowrap">
                    {#if r.carRef}<button class="mbtn gh !py-0.5" on:click={() => dispatch('goto', { tab: 'act' })}>{r.carRef}</button>
                    {:else if !r.met && d.can.car}<button class="mbtn danger !py-0.5" disabled={busy} on:click={() => raiseCar(r.period)}>เปิด CAR</button>{/if}
                    {#if d.can.record}<button class="mbtn gh !py-0.5" title="แก้ผลงวดนี้" on:click={() => startRecord(r.period)}><Icon name="pencil" size={13} /></button>{/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else}<p class="mut text-[13px]">ยังไม่มีผลที่บันทึก</p>{/if}
        <p class="mnote mt-2">สร้างโดย {d.createdName} · {fmtWhen(d.createdAt)} · แก้ไขล่าสุด {fmtWhen(d.updatedAt)}</p>
      </div>
    </div>
  </div>
{/if}

{#if f}
  <div class="sgbg" on:mousedown|self={() => (f = null)}>
    <div class="sgbox wide">
      <h3>{editing ? `แก้ไข ${editing.code}` : 'เพิ่มวัตถุประสงค์คุณภาพ'}</h3>
      <div class="sgs">ISO 9001 ข้อ 6.2.1: สอดคล้องนโยบายคุณภาพ วัดได้ ติดตาม สื่อสาร และปรับปรุงตามความเหมาะสม</div>
      <div class="sgf"><label>วัตถุประสงค์ / ตัวชี้วัด <em>*</em></label><input bind:value={f.title} placeholder="เช่น อัตราการส่งมอบตรงเวลา" /></div>
      <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
        <div class="sgf"><label>เงื่อนไข</label><select bind:value={f.direction}><option value="gte">≥ ไม่น้อยกว่า</option><option value="lte">≤ ไม่เกิน</option></select></div>
        <div class="sgf"><label>เป้าหมาย <em>*</em></label><input type="number" step="any" bind:value={f.target} /></div>
        <div class="sgf"><label>หน่วย</label><input bind:value={f.unit} placeholder="%, ครั้ง, วัน" /></div>
        <div class="sgf"><label>ความถี่</label><select bind:value={f.frequency} on:change={() => (f.startPeriod = '')}>{#each Object.entries(FREQ) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
      </div>
      <div class="grid grid-cols-2 md:grid-cols-3 gap-3">
        <div class="sgf"><label>แผนก</label><select bind:value={f.dept}><option value="">— ไม่ระบุ —</option>{#each DEPTS as dp}<option value={dp}>{DEPTNAME[dp]}</option>{/each}</select></div>
        <div class="sgf"><label>กระบวนการ</label><input bind:value={f.process} placeholder="เช่น การผลิต, จัดซื้อ" /></div>
        <div class="sgf"><label>เริ่มวัดตั้งแต่งวด</label><select bind:value={f.startPeriod}><option value="">— ทุกงวด —</option>{#each recentPeriods(f.frequency, 24) as p}<option value={p}>{periodLabel(p)}</option>{/each}</select></div>
      </div>
      <div class="sgf mr-person"><label>ผู้รับผิดชอบ (บันทึกผลรายงวด) <em>*</em></label>
        <input bind:value={pq} on:input={findPeople} placeholder="พิมพ์ชื่อหรือรหัสพนักงาน" />
        {#if f.ownerCode}<div class="mut text-[12px]">เลือก: {f.ownerName} ({f.ownerCode})</div>{/if}
        {#if people.length}<div class="mr-people">{#each people as p}<button on:click={() => pickPerson(p)}>{p.empCode} · {p.nameTh} <span class="mut">{p.position}</span></button>{/each}</div>{/if}
      </div>
      <div class="sgf"><label>วิธีคำนวณ / แหล่งข้อมูล</label><input bind:value={f.formula} placeholder="เช่น จำนวนใบส่งของตรงเวลา ÷ ทั้งหมด × 100 (จาก FM-WH-003)" /></div>
      <div class="sgf"><label>แผนการบรรลุ: ทำอะไร ใครรับผิดชอบ เสร็จเมื่อไร <em>*</em></label><textarea rows="3" bind:value={f.plan}></textarea></div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ทรัพยากรที่ต้องใช้</label><input bind:value={f.resources} /></div>
        <div class="sgf"><label>วิธีประเมินผล</label><input bind:value={f.evaluation} placeholder="เช่น ทบทวนรายเดือนในที่ประชุมฝ่าย" /></div>
      </div>
      {#if editing}<p class="mnote">เปลี่ยนเป้าหมายมีผลกับงวดที่บันทึกต่อจากนี้ — ผลงวดเดิมยังเทียบกับเป้าหมาย ณ ตอนบันทึก</p>{/if}
      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact"><button class="mbtn gh" on:click={() => (f = null)}>ยกเลิก</button><button class="mbtn accent" disabled={saving} on:click={saveForm}>{saving ? 'กำลังบันทึก…' : 'บันทึก'}</button></div>
    </div>
  </div>
{/if}
