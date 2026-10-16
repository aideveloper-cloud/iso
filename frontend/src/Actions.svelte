<script>
  // CAR / DAR / PAR ครบวงจร (ISO 9001:2015 ข้อ 10.2)
  //   เปิด → กำลังดำเนินการ → รอทวนสอบ → ปิด · ต้องมีการแก้ไขเฉพาะหน้า สาเหตุ การแก้ไขป้องกัน และทวนสอบประสิทธิผลโดยผู้อื่น
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import { fmtWhen } from './lib/changeNote.js'
  import DecisionDialog from './DecisionDialog.svelte'
  import Icon from './Icon.svelte'
  export let docs = []
  export let me = null

  const KINDS = ['CAR', 'DAR', 'PAR']
  const KIND_LABEL = { CAR: 'คำร้องขอแก้ไข (CAR)', DAR: 'ใบขอแก้ไขเอกสาร (DAR)', PAR: 'คำร้องขอป้องกัน (PAR)' }
  const STATUS_LABEL = { open: 'เปิด', in_progress: 'กำลังดำเนินการ', verify: 'รอทวนสอบ', closed: 'ปิดแล้ว', cancelled: 'ยกเลิก', overdue: 'เกินกำหนด' }
  const PRIO_LABEL = { high: 'สูง', medium: 'ปานกลาง', low: 'ต่ำ' }
  const SOURCE_LABEL = { internal_audit: 'ตรวจติดตามภายใน', external_audit: 'ตรวจประเมินภายนอก', customer: 'ข้อร้องเรียนลูกค้า', process: 'กระบวนการ/การผลิต', product: 'ผลิตภัณฑ์ไม่เป็นไปตามข้อกำหนด', supplier: 'ผู้ให้บริการภายนอก', mr: 'ทบทวนฝ่ายบริหาร', other: 'อื่น ๆ' }
  const RCA = { '5why': '5 Why', fishbone: 'ผังก้างปลา (4M/6M)', pareto: 'Pareto', other: 'อื่น ๆ' }
  const EVENT = { open: 'เปิดใบ', edit: 'แก้ไข', start: 'เริ่มดำเนินการ', submit: 'ส่งทวนสอบ (ลงนาม)', verify_effective: 'ทวนสอบ: มีประสิทธิผล → ปิด', verify_not_effective: 'ทวนสอบ: ไม่มีประสิทธิผล → กลับไปแก้ไข', cancel: 'ยกเลิก' }
  const STEPS = ['open', 'in_progress', 'verify', 'closed']

  let list = [], loading = true, err = ''
  let activeKind = 'all', statusFilter = 'active'
  let showForm = false, saving = false, formErr = '', msg = ''
  let d = null // ใบที่เปิดดู (จาก /api/actions/:id)
  let deciding = null // start | submit | verify_ok | verify_ng | cancel

  const today = () => new Date(Date.now() + 7 * 3600e3).toISOString().slice(0, 10)
  const isOverdue = (a) => ['open', 'in_progress'].includes(a.status) && a.dueDate && a.dueDate < today()
  const writer = () => me && me.role !== 'viewer'

  async function load() {
    loading = true; err = ''
    try { list = await api.actions() } catch (e) { err = 'โหลดรายการไม่สำเร็จ: ' + e.message }
    loading = false
  }
  onMount(load)

  $: kindCounts = { all: list.length, ...Object.fromEntries(KINDS.map((k) => [k, list.filter((a) => a.kind === k).length])) }
  $: base = list.filter((a) => activeKind === 'all' || a.kind === activeKind)
  const FILTERS = [
    ['active', 'ยังไม่ปิด', (a) => ['open', 'in_progress', 'verify'].includes(a.status)],
    ['mine', 'งานของฉัน', (a) => a.can?.start || a.can?.submit || a.can?.verify || (a.assigneeCode === me?.empCode && ['open', 'in_progress'].includes(a.status))],
    ['overdue', 'เกินกำหนด', isOverdue],
    ['verify', 'รอทวนสอบ', (a) => a.status === 'verify'],
    ['closed', 'ปิดแล้ว', (a) => a.status === 'closed'],
    ['cancelled', 'ยกเลิก', (a) => a.status === 'cancelled'],
    ['all', 'ทั้งหมด', () => true],
  ]
  $: counts = Object.fromEntries(FILTERS.map(([k, , fn]) => [k, base.filter(fn).length]))
  $: filtered = base.filter(FILTERS.find(([k]) => k === statusFilter)[2])

  // ---- เลือกพนักงาน (ผู้รับผิดชอบ) ----
  let people = [], pq = ''
  async function findPeople() {
    if (!pq.trim()) { people = []; return }
    try { people = (await api.employees(pq.trim())).slice(0, 8) } catch (e) { people = [] }
  }
  function pickPerson(target, p) { target.assigneeCode = p.empCode; target.assignee = p.nameTh; pq = p.nameTh; people = [] }

  // ---- เปิดใบใหม่ ----
  const blankForm = () => ({ kind: 'CAR', title: '', detail: '', source: '', dept: '', docCode: '', assigneeCode: '', assignee: '', priority: 'medium', dueDate: '' })
  let f = blankForm()
  function openForm() { f = blankForm(); formErr = ''; pq = ''; people = []; showForm = true }
  async function submitForm() {
    if (!f.title.trim()) { formErr = 'กรุณาระบุเรื่อง'; return }
    saving = true; formErr = ''
    const { assignee, ...payload } = f
    const { ok, body } = await api.actionCreate(payload)
    saving = false
    if (!ok) { formErr = body.error || 'เปิดใบไม่สำเร็จ'; return }
    showForm = false
    await load()
    openDetail(body.action)
  }

  // ---- รายละเอียด ----
  let e = null // ฟอร์มแก้ไข (คัดลอกจาก d)
  async function openDetail(a) {
    formErr = ''; msg = ''
    try {
      const r = await api.action(a.id)
      if (r.error) throw new Error(r.error)
      setDetail(r.action)
    } catch (x) { err = 'เปิดใบไม่สำเร็จ: ' + x.message }
  }
  function setDetail(a) { d = a; e = JSON.parse(JSON.stringify(a)); pq = a.assignee || ''; people = [] }
  const EDIT_KEYS = ['title', 'detail', 'source', 'dept', 'docCode', 'assigneeCode', 'priority', 'dueDate', 'containment', 'rcaMethod', 'rootCause', 'actionTaken', 'progress', 'verifyDue', 'riskReview', 'linkedRef']
  $: dirty = d && e && EDIT_KEYS.some((k) => String(e[k] ?? '') !== String(d[k] ?? ''))
  async function saveDetail() {
    saving = true; formErr = ''
    const fields = Object.fromEntries(EDIT_KEYS.filter((k) => String(e[k] ?? '') !== String(d[k] ?? '')).map((k) => [k, k === 'progress' ? Number(e[k]) : e[k]]))
    const { ok, body } = await api.actionUpdate(d.id, d.version, fields)
    saving = false
    if (!ok) { formErr = body.error || 'บันทึกไม่สำเร็จ'; return false }
    setDetail(body.action)
    msg = 'บันทึกแล้ว'
    load()
    return true
  }
  const DECIDE = {
    start: { title: 'เริ่มดำเนินการ', label: 'เริ่มดำเนินการ', comment: 'หมายเหตุ', noPin: true },
    submit: { title: 'ส่งทวนสอบประสิทธิผล (ลงนามผู้รับผิดชอบ)', label: 'ส่งทวนสอบ', comment: 'สรุปผลการดำเนินการ' },
    verify_ok: { title: 'ทวนสอบ: มีประสิทธิผล → ปิดใบ', label: 'ยืนยัน มีประสิทธิผล', comment: 'หลักฐาน/ผลการทวนสอบ (เช่น ติดตาม 3 เดือนไม่เกิดซ้ำ)', required: true },
    verify_ng: { title: 'ทวนสอบ: ไม่มีประสิทธิผล → กลับไปแก้ไข', label: 'ไม่มีประสิทธิผล', comment: 'ผลการทวนสอบและสิ่งที่ต้องแก้เพิ่ม', required: true, danger: true },
    cancel: { title: 'ยกเลิกใบ', label: 'ยกเลิกใบ', comment: 'เหตุผลการยกเลิก', required: true, danger: true, noPin: true },
  }
  $: dd = deciding ? DECIDE[deciding] : null
  async function onDecide(ev) {
    const { comment, pin, fail } = ev.detail
    if (dirty && !(await saveDetail())) { fail(formErr || 'บันทึกการแก้ไขก่อนไม่สำเร็จ'); return }
    const action = deciding.startsWith('verify') ? 'verify' : deciding
    const r = await api.actionAct(d.id, d.version, action, comment, pin, deciding === 'verify_ok' ? true : deciding === 'verify_ng' ? false : undefined)
    if (!r.ok) { fail(r.body.error || 'บันทึกไม่สำเร็จ'); return }
    const was = deciding
    deciding = null
    setDetail(r.body.action)
    msg = { start: 'เริ่มดำเนินการแล้ว', submit: 'ส่งทวนสอบแล้ว', verify_ok: 'ทวนสอบแล้ว — ปิดใบ', verify_ng: 'บันทึกว่าไม่มีประสิทธิผล — กลับไปดำเนินการต่อ', cancel: 'ยกเลิกใบแล้ว' }[was] || 'บันทึกแล้ว'
    load()
  }
  $: stepIdx = d ? STEPS.indexOf(d.status) : -1
  $: editable = !!d?.can?.edit
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="clipboard-check" size={20} /> CAR / DAR / PAR</h2>
  <p class="cat-desc">ISO 9001:2015 ข้อ 10.2 — แก้ไขเฉพาะหน้า → วิเคราะห์สาเหตุ → แก้ไขป้องกัน → ทวนสอบประสิทธิผลโดยผู้อื่น (ลงนามด้วย PIN) → ปิด</p>
</div>

<div class="type-tabs">
  <button class="type-tab {activeKind === 'all' ? 'active' : ''}" on:click={() => (activeKind = 'all')}><Icon name="files" size={15} /> ทั้งหมด <span class="tt-n">{kindCounts.all}</span></button>
  {#each KINDS as k}
    <button class="type-tab {activeKind === k ? 'active' : ''}" on:click={() => (activeKind = k)}><span class="kind-badge k-{k}">{k}</span> {KIND_LABEL[k].replace(/\s*\(.*\)/, '')} <span class="tt-n">{kindCounts[k]}</span></button>
  {/each}
  <span class="flex-1"></span>
  {#if writer()}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={openForm}><Icon name="plus" size={15} /> เปิดใบใหม่</button>{/if}
</div>

<div class="dept-chips">
  {#each FILTERS as [k, label]}
    <button class="dept-chip {statusFilter === k ? 'active' : ''}" on:click={() => (statusFilter = k)}>{label} <span class="dc-n">{counts[k]}</span></button>
  {/each}
</div>

{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}

{#if loading}
  <div class="loadwrap"><span class="spin"></span><p class="note m-0">กำลังโหลด…</p></div>
{:else if !filtered.length}
  <div class="empty-state"><Icon name="clipboard-check" size={38} stroke={1.5} /><p>ไม่มีใบในหมวดนี้</p></div>
{:else}
  <div class="act-grid">
    {#each filtered as a (a.id)}
      {@const ov = isOverdue(a)}
      <div class="act-card" role="button" tabindex="0" on:click={() => openDetail(a)} on:keydown={(k) => k.key === 'Enter' && openDetail(a)}>
        <div class="ac-top">
          <span class="kind-badge k-{a.kind}">{a.kind}</span><span class="ac-ref">{a.refNo}</span><span class="flex-1"></span>
          <span class="status-badge s-{ov ? 'overdue' : a.status}">{STATUS_LABEL[ov ? 'overdue' : a.status]}</span>
        </div>
        <div class="ac-title">{a.title}</div>
        <div class="ac-meta">
          {#if a.assignee}<span class="inline-flex items-center gap-1"><Icon name="user" size={12} /> {a.assignee}</span>{/if}
          {#if a.source}<span>· {SOURCE_LABEL[a.source]}</span>{/if}
          {#if a.dueDate}<span class="inline-flex items-center gap-1 {ov ? 'text-danger font-bold' : ''}"><Icon name="calendar" size={12} /> {a.dueDate}</span>{/if}
          <span class="prio p-{a.priority}">ความสำคัญ: {PRIO_LABEL[a.priority] || a.priority}</span>
        </div>
        <div class="prog"><div class="prog-fill" style="width:{a.status === 'closed' ? 100 : a.progress}%"></div></div>
        <div class="ac-prog-n">{a.status === 'closed' ? `ปิดแล้ว · ทวนสอบโดย ${a.verifiedBy || '—'}` : `ความคืบหน้า ${a.progress}%`}</div>
      </div>
    {/each}
  </div>
{/if}

{#if showForm}
  <div class="sgbg" on:mousedown|self={() => (showForm = false)}>
    <div class="sgbox wide">
      <h3>เปิดใบใหม่</h3>
      <div class="sgs">ผู้เปิดใบ: <b>{me?.name}</b> · เลขที่ใบสร้างอัตโนมัติ</div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ประเภท <em>*</em></label><select bind:value={f.kind}>{#each KINDS as k}<option value={k}>{KIND_LABEL[k]}</option>{/each}</select></div>
        <div class="sgf"><label>แหล่งที่มา</label><select bind:value={f.source}><option value="">— ไม่ระบุ —</option>{#each Object.entries(SOURCE_LABEL) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
      </div>
      <div class="sgf"><label>เรื่อง <em>*</em></label><input bind:value={f.title} placeholder="สรุปปัญหา/สิ่งที่ไม่เป็นไปตามข้อกำหนด" /></div>
      <div class="sgf"><label>รายละเอียดปัญหา (พบที่ไหน เมื่อไร เท่าไร)</label><textarea bind:value={f.detail} rows="3"></textarea></div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>แผนก</label><select bind:value={f.dept}><option value="">— ไม่ระบุ —</option>{#each DEPTS as dp}<option value={dp}>{DEPTNAME[dp]}</option>{/each}</select></div>
        <div class="sgf"><label>เอกสารอ้างอิง</label><select bind:value={f.docCode}><option value="">— ไม่ระบุ —</option>{#each docs as doc}<option value={doc.code}>{doc.code} · {doc.name}</option>{/each}</select></div>
      </div>
      <div class="sgf mr-person"><label>ผู้รับผิดชอบ (พนักงานในระบบ)</label>
        <input bind:value={pq} on:input={findPeople} placeholder="พิมพ์ชื่อหรือรหัสพนักงาน" />
        {#if f.assigneeCode}<div class="mut text-[12px]">เลือก: {f.assignee} ({f.assigneeCode})</div>{/if}
        {#if people.length}<div class="mr-people">{#each people as p}<button on:click={() => pickPerson(f, p)}>{p.empCode} · {p.nameTh} <span class="mut">{p.position}</span></button>{/each}</div>{/if}
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ความสำคัญ</label><select bind:value={f.priority}><option value="high">สูง</option><option value="medium">ปานกลาง</option><option value="low">ต่ำ</option></select></div>
        <div class="sgf"><label>กำหนดแล้วเสร็จ</label><input type="date" bind:value={f.dueDate} /></div>
      </div>
      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact"><button class="mbtn gh" on:click={() => (showForm = false)}>ยกเลิก</button><button class="mbtn accent" disabled={saving} on:click={submitForm}>{saving ? 'กำลังบันทึก…' : 'เปิดใบ'}</button></div>
    </div>
  </div>
{/if}

{#if d && e}
  <div class="mbg" on:mousedown|self={() => (d = null)}>
    <div class="mbox wide">
      <header class="mhead">
        <div class="mt"><span class="kind-badge k-{d.kind}">{d.kind}</span> <b>{d.refNo}</b> {d.title}
          <div class="msub">เปิดโดย {d.raisedBy} · {fmtWhen(d.createdAt)}{#if d.source} · {SOURCE_LABEL[d.source]}{/if}</div>
        </div>
        <button class="mx inline-flex items-center justify-center" on:click={() => (d = null)}><Icon name="x" size={16} /></button>
      </header>
      <div class="mbody">
        <div class="mr-steps">
          {#each STEPS as stp, i}<div class="mr-step" class:done={stepIdx > i || d.status === 'closed'} class:cur={stepIdx === i && d.status !== 'closed'}><span class="mr-step-n">{#if stepIdx > i || d.status === 'closed'}<Icon name="circle-check" size={14} />{:else}{i + 1}{/if}</span>{STATUS_LABEL[stp]}</div>{/each}
          {#if d.status === 'cancelled'}<div class="mr-step cur">ยกเลิก: {d.cancelReason}</div>{/if}
        </div>
        {#if msg}<div class="save-ok">{msg}</div>{/if}
        {#if d.effectiveness === 'not_effective' && d.status === 'in_progress'}<div class="merr">ทวนสอบครั้งก่อน “ไม่มีประสิทธิผล” โดย {d.verifiedBy}: {d.verifyNote}</div>{/if}

        <div class="mr-bar">
          {#if d.can.start}<button class="mbtn accent" on:click={() => (deciding = 'start')}>เริ่มดำเนินการ</button>{/if}
          {#if d.can.submit}<button class="mbtn accent" disabled={d.gaps.length && !dirty} title={d.gaps.length ? 'ยังขาด: ' + d.gaps.join(', ') : ''} on:click={() => (deciding = 'submit')}><Icon name="pen-line" size={14} /> ส่งทวนสอบ</button>{/if}
          {#if d.can.verify}
            <button class="mbtn accent" on:click={() => (deciding = 'verify_ok')}>มีประสิทธิผล · ปิดใบ</button>
            <button class="mbtn danger" on:click={() => (deciding = 'verify_ng')}>ไม่มีประสิทธิผล</button>
          {/if}
          {#if d.can.cancel}<button class="mbtn gh" on:click={() => (deciding = 'cancel')}>ยกเลิกใบ</button>{/if}
          {#if d.status === 'verify' && !d.can.verify}<span class="mnote">รอผู้ตรวจสอบ (ที่ไม่ใช่ผู้รับผิดชอบ) ทวนสอบประสิทธิผล</span>{/if}
          {#if d.can.submit && d.gaps.length}<span class="mnote">ก่อนส่งทวนสอบต้องกรอก: {d.gaps.join(', ')}</span>{/if}
        </div>

        <div class="capa-grid">
          <section class="sec">
            <h2 class="!mb-2">1. ปัญหา</h2>
            <div class="sgf"><label>เรื่อง</label><input bind:value={e.title} disabled={!editable} /></div>
            <div class="sgf"><label>รายละเอียด</label><textarea rows="3" bind:value={e.detail} disabled={!editable}></textarea></div>
            <div class="grid grid-cols-2 gap-3">
              <div class="sgf"><label>แหล่งที่มา</label><select bind:value={e.source} disabled={!editable}><option value="">—</option>{#each Object.entries(SOURCE_LABEL) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
              <div class="sgf"><label>แผนก</label><select bind:value={e.dept} disabled={!editable}><option value="">—</option>{#each DEPTS as dp}<option value={dp}>{DEPTNAME[dp]}</option>{/each}</select></div>
              <div class="sgf"><label>เอกสารอ้างอิง</label><select bind:value={e.docCode} disabled={!editable}><option value="">—</option>{#each docs as doc}<option value={doc.code}>{doc.code}</option>{/each}</select></div>
              <div class="sgf"><label>อ้างอิงอื่น (DCR/MR)</label><input bind:value={e.linkedRef} disabled={!editable} placeholder="เช่น DCR-2569-0003" /></div>
            </div>
          </section>
          <section class="sec">
            <h2 class="!mb-2">ผู้รับผิดชอบและกำหนดเวลา</h2>
            <div class="sgf mr-person"><label>ผู้รับผิดชอบ</label>
              <input bind:value={pq} on:input={findPeople} disabled={!editable} placeholder="พิมพ์ชื่อหรือรหัสพนักงาน" />
              {#if e.assigneeCode}<div class="mut text-[12px]">{e.assignee} ({e.assigneeCode})</div>{/if}
              {#if people.length}<div class="mr-people">{#each people as p}<button on:click={() => pickPerson(e, p)}>{p.empCode} · {p.nameTh} <span class="mut">{p.position}</span></button>{/each}</div>{/if}
            </div>
            <div class="grid grid-cols-2 gap-3">
              <div class="sgf"><label>ความสำคัญ</label><select bind:value={e.priority} disabled={!editable}><option value="high">สูง</option><option value="medium">ปานกลาง</option><option value="low">ต่ำ</option></select></div>
              <div class="sgf"><label>กำหนดแล้วเสร็จ</label><input type="date" bind:value={e.dueDate} disabled={!editable} /></div>
              <div class="sgf"><label>กำหนดทวนสอบประสิทธิผล</label><input type="date" bind:value={e.verifyDue} disabled={!editable} /></div>
              <div class="sgf"><label>ความคืบหน้า: {d.status === 'closed' ? 100 : e.progress}%</label><input type="range" min="0" max="99" step="5" bind:value={e.progress} disabled={!editable} /></div>
            </div>
          </section>
        </div>

        <section class="sec mt-3">
          <h2 class="!mb-2">2. การแก้ไขเฉพาะหน้า (Containment / Correction){#if d.kind === 'CAR'} <em class="text-danger not-italic">*</em>{/if}</h2>
          <textarea class="capa-ta" rows="2" bind:value={e.containment} disabled={!editable} placeholder="กักกัน/คัดแยกของเสีย แจ้งลูกค้า แก้ไขทันทีเพื่อหยุดผลกระทบ"></textarea>
        </section>
        <section class="sec mt-3">
          <h2 class="!mb-2">3. วิเคราะห์สาเหตุ{#if d.kind !== 'DAR'} <em class="text-danger not-italic">*</em>{/if}</h2>
          <div class="sgf !mb-2"><label>วิธีวิเคราะห์</label><select bind:value={e.rcaMethod} disabled={!editable}><option value="">—</option>{#each Object.entries(RCA) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
          <textarea class="capa-ta" rows="3" bind:value={e.rootCause} disabled={!editable} placeholder={d.kind === 'PAR' ? 'สาเหตุที่อาจทำให้เกิดปัญหา' : 'สาเหตุที่แท้จริง (ถาม ทำไม ซ้ำจนถึงต้นเหตุ)'}></textarea>
        </section>
        <section class="sec mt-3">
          <h2 class="!mb-2">4. การแก้ไข/ป้องกันไม่ให้เกิดซ้ำ <em class="text-danger not-italic">*</em></h2>
          <textarea class="capa-ta" rows="3" bind:value={e.actionTaken} disabled={!editable} placeholder="มาตรการที่กำจัดสาเหตุ เช่น แก้ WI อบรม เปลี่ยนวิธีตรวจ (ถ้าต้องแก้เอกสาร ให้ส่ง DCR และใส่เลขที่ในช่องอ้างอิง)"></textarea>
          <div class="sgf !mt-2 !mb-0"><label>ทบทวนความเสี่ยง/ระบบ (10.2.1 e–f)</label><input bind:value={e.riskReview} disabled={!editable} placeholder="ปรับทะเบียนความเสี่ยงหรือเปลี่ยนแปลงระบบที่เกี่ยวข้อง (ถ้ามี)" /></div>
        </section>
        <section class="sec mt-3">
          <h2 class="!mb-2">5. ทวนสอบประสิทธิผล</h2>
          {#if d.verifiedAt}
            <div class="det-info"><div><b>ผล:</b> {d.effectiveness === 'effective' ? 'มีประสิทธิผล' : 'ไม่มีประสิทธิผล'}</div><div><b>โดย:</b> {d.verifiedBy} · {fmtWhen(d.verifiedAt)}</div><div class="col-span-2"><b>หลักฐาน:</b> {d.verifyNote}</div></div>
          {:else}<p class="mut text-[12.5px] m-0">ยังไม่ทวนสอบ — ผู้ตรวจสอบที่ไม่ใช่ผู้รับผิดชอบเป็นผู้ทวนสอบหลังส่งงาน</p>{/if}
        </section>

        {#if formErr}<div class="merr mt-3">{formErr}</div>{/if}
        {#if editable}<div class="sgact !justify-start mt-3"><button class="mbtn accent" disabled={!dirty || saving} on:click={saveDetail}>{saving ? 'กำลังบันทึก…' : 'บันทึกการแก้ไข'}</button></div>{/if}

        {#if d.steps?.length}
          <h3 class="rd-sub">ประวัติ</h3>
          <table class="chg-table"><thead><tr><th>เมื่อ</th><th>ผู้ดำเนินการ</th><th>การดำเนินการ</th><th>รายละเอียด</th></tr></thead>
            <tbody>{#each d.steps as s}<tr><td class="nowrap">{fmtWhen(s.at)}</td><td>{s.actorName}</td><td>{EVENT[s.event] || s.event}</td><td>{s.detail || '—'}</td></tr>{/each}</tbody></table>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if dd}
  <DecisionDialog title={dd.title} message="{d.refNo} · {d.title}" {me} commentLabel={dd.comment} commentRequired={!!dd.required}
    withPin={!dd.noPin} confirmLabel={dd.label} danger={!!dd.danger} on:confirm={onDecide} on:cancel={() => (deciding = null)} />
{/if}
