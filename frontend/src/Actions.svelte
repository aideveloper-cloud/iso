<script>
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import Icon from './Icon.svelte'
  export let docs = []

  const KINDS = ['CAR', 'DAR', 'PAR']
  const KIND_LABEL = { CAR: 'คำร้องขอแก้ไข (CAR)', DAR: 'ใบขอแก้ไขเอกสาร (DAR)', PAR: 'คำร้องขอป้องกัน (PAR)' }
  const STATUS_LABEL = { open: 'เปิด', in_progress: 'กำลังดำเนินการ', closed: 'ปิดแล้ว', overdue: 'เกินกำหนด' }
  const PRIO_LABEL = { high: 'สูง', medium: 'ปานกลาง', low: 'ต่ำ' }

  let list = [], loading = true, err = ''
  let activeKind = 'all', statusFilter = 'all'
  let showForm = false, detail = null, saving = false, formErr = ''

  const today = () => new Date().toISOString().slice(0, 10)
  const isOverdue = (a) => a.status !== 'closed' && a.dueDate && a.dueDate < today()
  const fmtDate = (s) => (s ? s : '—')

  async function load() {
    loading = true; err = ''
    try { list = await api.actions() } catch (e) { err = 'โหลดรายการไม่สำเร็จ: ' + e.message }
    loading = false
  }
  onMount(load)

  $: kindCounts = { all: list.length, CAR: list.filter((a) => a.kind === 'CAR').length, DAR: list.filter((a) => a.kind === 'DAR').length, PAR: list.filter((a) => a.kind === 'PAR').length }
  $: base = list.filter((a) => activeKind === 'all' || a.kind === activeKind)
  $: statusCounts = {
    all: base.length,
    open: base.filter((a) => a.status === 'open' && !isOverdue(a)).length,
    in_progress: base.filter((a) => a.status === 'in_progress' && !isOverdue(a)).length,
    overdue: base.filter(isOverdue).length,
    closed: base.filter((a) => a.status === 'closed').length,
  }
  $: filtered = base.filter((a) => {
    if (statusFilter === 'all') return true
    if (statusFilter === 'overdue') return isOverdue(a)
    if (statusFilter === 'closed') return a.status === 'closed'
    return a.status === statusFilter && !isOverdue(a)
  })

  // ---- ฟอร์มเปิดใบใหม่ ----
  const blankForm = () => ({ kind: 'CAR', title: '', detail: '', dept: '', docCode: '', raisedBy: '', assignee: '', priority: 'medium', dueDate: '' })
  let f = blankForm()
  function openForm() { f = blankForm(); formErr = ''; showForm = true }
  async function submitForm() {
    if (!f.title.trim()) { formErr = 'กรุณาระบุเรื่อง'; return }
    saving = true; formErr = ''
    const { ok, body } = await api.actionCreate(f)
    saving = false
    if (!ok) { formErr = body.error || 'เปิดใบไม่สำเร็จ'; return }
    showForm = false; await load()
  }

  // ---- รายละเอียด / อัปเดต ----
  let d = null
  function openDetail(a) { d = { ...a }; detail = a; formErr = '' }
  async function saveDetail() {
    saving = true; formErr = ''
    const { ok, body } = await api.actionUpdate(d.id, {
      status: d.status, progress: Number(d.progress), actionTaken: d.actionTaken,
      assignee: d.assignee, dueDate: d.dueDate, priority: d.priority,
    })
    saving = false
    if (!ok) { formErr = body.error || 'อัปเดตไม่สำเร็จ'; return }
    detail = null; await load()
  }
  function setStatus(s) { d.status = s; if (s === 'closed') d.progress = 100 }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="clipboard-check" size={20} /> ติดตาม CAR / DAR / PAR</h2>
  <p class="cat-desc">เปิดใบร้องขอแก้ไข (CAR) · ใบขอแก้ไขเอกสาร (DAR) · ใบร้องขอป้องกัน (PAR) — กำหนดผู้รับผิดชอบ, กำหนดเสร็จ และติดตามสถานะแบบเรียลไทม์</p>
</div>

<!-- แท็บประเภทใบ -->
<div class="type-tabs">
  <button class="type-tab {activeKind === 'all' ? 'active' : ''}" on:click={() => (activeKind = 'all')}>
    <Icon name="files" size={15} /> ทั้งหมด <span class="tt-n">{kindCounts.all}</span>
  </button>
  {#each KINDS as k}
    <button class="type-tab {activeKind === k ? 'active' : ''}" on:click={() => (activeKind = k)}>
      <span class="kind-badge k-{k}">{k}</span> {KIND_LABEL[k].replace(/\s*\(.*\)/, '')} <span class="tt-n">{kindCounts[k]}</span>
    </button>
  {/each}
  <span class="flex-1"></span>
  <button class="mbtn accent inline-flex items-center gap-1.5" on:click={openForm}><Icon name="plus" size={15} /> เปิดใบใหม่</button>
</div>

<!-- ชิปกรองสถานะ -->
<div class="dept-chips">
  <button class="dept-chip {statusFilter === 'all' ? 'active' : ''}" on:click={() => (statusFilter = 'all')}>ทั้งหมด <span class="dc-n">{statusCounts.all}</span></button>
  <button class="dept-chip {statusFilter === 'open' ? 'active' : ''}" on:click={() => (statusFilter = 'open')}>เปิด <span class="dc-n">{statusCounts.open}</span></button>
  <button class="dept-chip {statusFilter === 'in_progress' ? 'active' : ''}" on:click={() => (statusFilter = 'in_progress')}>กำลังดำเนินการ <span class="dc-n">{statusCounts.in_progress}</span></button>
  <button class="dept-chip {statusFilter === 'overdue' ? 'active' : ''}" on:click={() => (statusFilter = 'overdue')}>เกินกำหนด <span class="dc-n">{statusCounts.overdue}</span></button>
  <button class="dept-chip {statusFilter === 'closed' ? 'active' : ''}" on:click={() => (statusFilter = 'closed')}>ปิดแล้ว <span class="dc-n">{statusCounts.closed}</span></button>
</div>

{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}

{#if loading}
  <div class="loadwrap"><span class="spin"></span><p class="note m-0">กำลังโหลด…</p></div>
{:else if !filtered.length}
  <div class="empty-state">
    <Icon name="clipboard-check" size={38} stroke={1.5} />
    <p>ยังไม่มีใบในหมวดนี้</p>
    <button class="mbtn accent inline-flex items-center gap-1.5" on:click={openForm}><Icon name="plus" size={15} /> เปิดใบใหม่</button>
  </div>
{:else}
  <div class="act-grid">
    {#each filtered as a (a.id)}
      {@const ov = isOverdue(a)}
      <div class="act-card" role="button" tabindex="0" on:click={() => openDetail(a)} on:keydown={(e) => e.key === 'Enter' && openDetail(a)}>
        <div class="ac-top">
          <span class="kind-badge k-{a.kind}">{a.kind}</span>
          <span class="ac-ref">{a.refNo}</span>
          <span class="flex-1"></span>
          <span class="status-badge s-{ov ? 'overdue' : a.status}">{STATUS_LABEL[ov ? 'overdue' : a.status]}</span>
        </div>
        <div class="ac-title">{a.title}</div>
        <div class="ac-meta">
          {#if a.assignee}<span class="inline-flex items-center gap-1"><Icon name="user" size={12} /> {a.assignee}</span>{/if}
          {#if a.dept}<span>· {DEPTNAME[a.dept] || a.dept}</span>{/if}
          {#if a.dueDate}<span class="inline-flex items-center gap-1 {ov ? 'text-danger font-bold' : ''}"><Icon name="calendar" size={12} /> {a.dueDate}</span>{/if}
          <span class="prio p-{a.priority}">ความสำคัญ: {PRIO_LABEL[a.priority] || a.priority}</span>
        </div>
        <div class="prog"><div class="prog-fill" style="width:{a.progress}%"></div></div>
        <div class="ac-prog-n">ความคืบหน้า {a.progress}%</div>
      </div>
    {/each}
  </div>
{/if}

<div class="note">แสดง {filtered.length} จาก {list.length} ใบ</div>

<!-- ฟอร์มเปิดใบใหม่ -->
{#if showForm}
  <div class="sgbg" on:mousedown|self={() => (showForm = false)}>
    <div class="sgbox" style="max-width:560px">
      <h3>เปิดใบร้องขอใหม่</h3>
      <div class="sgs">กรอกรายละเอียดเพื่อเปิดใบ CAR / DAR / PAR — เลขที่ใบจะสร้างอัตโนมัติ</div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ประเภท <em>*</em></label>
          <select bind:value={f.kind}>{#each KINDS as k}<option value={k}>{KIND_LABEL[k]}</option>{/each}</select></div>
        <div class="sgf"><label>ความสำคัญ</label>
          <select bind:value={f.priority}><option value="high">สูง</option><option value="medium">ปานกลาง</option><option value="low">ต่ำ</option></select></div>
      </div>
      <div class="sgf"><label>เรื่อง <em>*</em></label><input bind:value={f.title} placeholder="สรุปสั้น ๆ ของประเด็น/ปัญหา" /></div>
      <div class="sgf"><label>รายละเอียด</label><textarea bind:value={f.detail} rows="2" placeholder="อธิบายปัญหา/สิ่งที่ต้องดำเนินการ"></textarea></div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>แผนกที่เกี่ยวข้อง</label>
          <select bind:value={f.dept}><option value="">— ไม่ระบุ —</option>{#each DEPTS as dp}<option value={dp}>{DEPTNAME[dp]}</option>{/each}</select></div>
        <div class="sgf"><label>เอกสารอ้างอิง</label>
          <select bind:value={f.docCode}><option value="">— ไม่ระบุ —</option>{#each docs as doc}<option value={doc.code}>{doc.code} · {doc.name}</option>{/each}</select></div>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ผู้เปิดใบ</label><input bind:value={f.raisedBy} placeholder="ชื่อ-สกุล" /></div>
        <div class="sgf"><label>ผู้รับผิดชอบ</label><input bind:value={f.assignee} placeholder="ชื่อ-สกุล" /></div>
      </div>
      <div class="sgf"><label>กำหนดแล้วเสร็จ</label><input type="date" bind:value={f.dueDate} /></div>
      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact">
        <button class="mbtn gh" on:click={() => (showForm = false)}>ยกเลิก</button>
        <button class="mbtn accent inline-flex items-center gap-1.5" disabled={saving} on:click={submitForm}><Icon name="plus" size={14} /> {saving ? 'กำลังบันทึก…' : 'เปิดใบ'}</button>
      </div>
    </div>
  </div>
{/if}

<!-- รายละเอียด / อัปเดตสถานะ -->
{#if detail && d}
  <div class="sgbg" on:mousedown|self={() => (detail = null)}>
    <div class="sgbox" style="max-width:600px">
      <div class="flex items-center gap-2 mb-1">
        <span class="kind-badge k-{d.kind}">{d.kind}</span>
        <h3 class="!m-0">{d.refNo}</h3>
      </div>
      <div class="ac-title !text-[15px] mb-1">{d.title}</div>
      {#if d.detail}<div class="sgs !mb-3">{d.detail}</div>{/if}

      <div class="det-info">
        {#if d.dept}<div><b>แผนก:</b> {DEPTNAME[d.dept] || d.dept}</div>{/if}
        {#if d.docCode}<div><b>เอกสารอ้างอิง:</b> {d.docCode}</div>{/if}
        {#if d.raisedBy}<div><b>ผู้เปิด:</b> {d.raisedBy}</div>{/if}
        <div><b>เปิดเมื่อ:</b> {d.createdAt ? new Date(d.createdAt).toLocaleDateString('th-TH') : '—'}</div>
        {#if d.closedAt}<div><b>ปิดเมื่อ:</b> {new Date(d.closedAt).toLocaleDateString('th-TH')}</div>{/if}
      </div>

      <div class="grid grid-cols-2 gap-3 mt-2">
        <div class="sgf"><label>ผู้รับผิดชอบ</label><input bind:value={d.assignee} placeholder="ชื่อ-สกุล" /></div>
        <div class="sgf"><label>กำหนดแล้วเสร็จ</label><input type="date" bind:value={d.dueDate} /></div>
      </div>

      <div class="sgf"><label>สถานะ</label>
        <div class="status-switch">
          <button class="ss-btn {d.status === 'open' ? 'active' : ''}" on:click={() => setStatus('open')}>เปิด</button>
          <button class="ss-btn {d.status === 'in_progress' ? 'active' : ''}" on:click={() => setStatus('in_progress')}>กำลังดำเนินการ</button>
          <button class="ss-btn {d.status === 'closed' ? 'active' : ''}" on:click={() => setStatus('closed')}>ปิด</button>
        </div>
      </div>

      <div class="sgf"><label>ความคืบหน้า: {d.progress}%</label>
        <input type="range" min="0" max="100" step="5" bind:value={d.progress} /></div>

      <div class="sgf"><label>การดำเนินการ / วิธีแก้ไข</label>
        <textarea bind:value={d.actionTaken} rows="3" placeholder="บันทึกสิ่งที่ดำเนินการไปแล้ว"></textarea></div>

      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact">
        <button class="mbtn gh" on:click={() => (detail = null)}>ปิด</button>
        <button class="mbtn accent inline-flex items-center gap-1.5" disabled={saving} on:click={saveDetail}><Icon name="circle-check" size={14} /> {saving ? 'กำลังบันทึก…' : 'บันทึก'}</button>
      </div>
    </div>
  </div>
{/if}
