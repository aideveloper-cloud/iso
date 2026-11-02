<script>
  // ทะเบียนความเสี่ยงและโอกาส (ISO 9001:2015 ข้อ 6.1 · บริบท 4.1 · ผู้มีส่วนได้ส่วนเสีย 4.2)
  //   ประเมิน โอกาสเกิด × ผลกระทบ → แผนจัดการ + ผู้รับผิดชอบ → ทบทวนตามกำหนด (ประเมินซ้ำ + ประสิทธิผล) → PAR เมื่อต้องมีมาตรการเชิงระบบ
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import { fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'
  export let me = null
  export let focus = '' // เลขที่รายการที่ต้องการเปิด (จากงานของฉัน)
  const dispatch = createEventDispatcher()

  const KIND = { risk: 'ความเสี่ยง', opportunity: 'โอกาส' }
  const CONTEXT = { internal: 'ประเด็นภายใน (4.1)', external: 'ประเด็นภายนอก (4.1)', party: 'ผู้มีส่วนได้ส่วนเสีย (4.2)', process: 'กระบวนการ', other: 'อื่น ๆ' }
  const TREAT = {
    risk: { avoid: 'หลีกเลี่ยง', reduce: 'ลดความเสี่ยง', transfer: 'ถ่ายโอน (ประกัน/ผู้รับจ้าง)', accept: 'ยอมรับ' },
    opportunity: { exploit: 'คว้าโอกาส', enhance: 'เพิ่มโอกาส', accept: 'ยอมรับ/เฝ้าติดตาม' },
  }
  const LEVEL = { high: 'สูง', medium: 'กลาง', low: 'ต่ำ' }
  const STATUS = { open: 'รอทบทวน', monitoring: 'ติดตาม', closed: 'ปิดแล้ว' }
  const EFF = { pending: 'ยังไม่ประเมิน', effective: 'มีประสิทธิผล', not_effective: 'ไม่มีประสิทธิผล' }
  const EVENT = { create: 'บันทึก', edit: 'แก้ไข', review: 'ทบทวน', close: 'ปิด', reopen: 'เปิดใหม่', par: 'เปิด PAR' }
  const L_TEXT = ['', 'น้อยมาก', 'น้อย', 'ปานกลาง', 'สูง', 'สูงมาก']
  const I_TEXT = ['', 'น้อยมาก', 'น้อย', 'ปานกลาง', 'มาก', 'รุนแรง']
  const lvl = (s) => (s >= 15 ? 'high' : s >= 8 ? 'medium' : s > 0 ? 'low' : '')
  const today = () => new Date(Date.now() + 7 * 3600e3).toISOString().slice(0, 10)
  const plusDays = (n) => new Date(Date.now() + 7 * 3600e3 + n * 864e5).toISOString().slice(0, 10)

  let list = [], loading = true, err = ''
  let kind = 'all', filter = 'open', cell = null
  async function load() {
    loading = true; err = ''
    try {
      const r = await api.risks()
      if (r.error) throw new Error(r.error)
      list = r
      if (focus) { const x = list.find((y) => y.refNo === focus); if (x) openDetail(x); focus = '' }
    } catch (e) { err = 'โหลดทะเบียนไม่สำเร็จ: ' + e.message }
    loading = false
  }
  onMount(load)

  const FILTERS = [
    ['open', 'ยังติดตาม', (r) => r.status !== 'closed'],
    ['mine', 'ที่ฉันรับผิดชอบ', (r) => r.status !== 'closed' && r.ownerCode === me?.empCode],
    ['high', 'ระดับสูง', (r) => r.status !== 'closed' && r.level === 'high'],
    ['due', 'เลยกำหนดทบทวน', (r) => r.reviewOverdue],
    ['gaps', 'ข้อมูลไม่ครบ', (r) => r.status !== 'closed' && r.gaps.length],
    ['closed', 'ปิดแล้ว', (r) => r.status === 'closed'],
    ['all', 'ทั้งหมด', () => true],
  ]
  $: base = list.filter((r) => kind === 'all' || r.kind === kind)
  $: counts = Object.fromEntries(FILTERS.map(([k, , fn]) => [k, base.filter(fn).length]))
  $: shown = base.filter(FILTERS.find(([k]) => k === filter)[2]).filter((r) => !cell || (r.likelihood === cell[0] && r.impact === cell[1]))
  // ผังความร้อน: นับรายการที่ยังติดตามในแต่ละช่อง โอกาสเกิด (แนวตั้ง) × ผลกระทบ (แนวนอน)
  $: heat = (() => {
    const m = {}
    for (const r of base) if (r.status !== 'closed') { const k = r.likelihood + ':' + r.impact; m[k] = (m[k] || 0) + 1 }
    return m
  })()
  $: openN = list.filter((r) => r.status !== 'closed')

  // ---- เลือกพนักงาน ----
  let people = [], pq = ''
  async function findPeople() {
    if (!pq.trim()) { people = []; return }
    try { people = (await api.employees(pq.trim())).slice(0, 8) } catch (e) { people = [] }
  }
  function pickPerson(p) { f.ownerCode = p.empCode; f.ownerName = p.nameTh; pq = p.nameTh; people = [] }

  // ---- บันทึก/แก้ไข ----
  const KEYS = ['title', 'detail', 'context', 'party', 'process', 'dept', 'ownerCode', 'likelihood', 'impact', 'existingControls', 'treatment', 'plan', 'residualLikelihood', 'residualImpact', 'reviewDue']
  const blank = (k = 'risk') => ({ kind: k, title: '', detail: '', context: '', party: '', process: '', dept: '', ownerCode: '', ownerName: '', likelihood: 3, impact: 3, existingControls: '', treatment: '', plan: '', residualLikelihood: 0, residualImpact: 0, reviewDue: '' })
  let f = null, editing = null, saving = false, formErr = ''
  function openCreate(k) { f = blank(k); editing = null; formErr = ''; pq = ''; people = [] }
  function openEdit() { f = { ...d }; editing = d; formErr = ''; pq = d.ownerName || ''; people = [] }
  $: fScore = f ? f.likelihood * f.impact : 0
  $: fResidual = f ? f.residualLikelihood * f.residualImpact : 0
  async function saveForm() {
    if (!f.title.trim()) { formErr = 'กรุณาระบุความเสี่ยง/โอกาส'; return }
    saving = true; formErr = ''
    const fields = Object.fromEntries(KEYS.map((k) => [k, ['likelihood', 'impact', 'residualLikelihood', 'residualImpact'].includes(k) ? Number(f[k]) : f[k]]))
    const r = editing ? await api.riskUpdate(editing.id, editing.version, fields) : await api.riskCreate({ ...fields, kind: f.kind })
    saving = false
    if (!r.ok) { formErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    f = null
    d = r.body.risk
    msg = editing ? 'บันทึกการแก้ไขแล้ว' : `บันทึก ${r.body.risk.refNo} แล้ว`
    load()
  }

  // ---- รายละเอียด / ทบทวน ----
  let d = null, msg = '', rv = null, rvErr = '', busy = false
  async function openDetail(x) {
    msg = ''; rv = null
    const r = await api.risk(x.id)
    if (r.error) { err = r.error; return }
    d = r.risk
  }
  function startReview() {
    rv = { likelihood: d.likelihood, impact: d.impact, effectiveness: d.effectiveness === 'pending' ? 'effective' : d.effectiveness, note: '', nextReview: plusDays(d.level === 'high' ? 90 : 365) }
    rvErr = ''
  }
  async function saveReview() {
    if (!rv.note.trim()) { rvErr = 'กรุณาบันทึกผลการทบทวน/หลักฐาน'; return }
    busy = true; rvErr = ''
    const r = await api.riskReview(d.id, { version: d.version, likelihood: Number(rv.likelihood), impact: Number(rv.impact), effectiveness: rv.effectiveness, note: rv.note, nextReview: rv.nextReview })
    busy = false
    if (!r.ok) { rvErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    d = r.body.risk; rv = null; msg = 'บันทึกการทบทวนแล้ว'
    load()
  }
  async function act(action) {
    const reason = prompt(action === 'close' ? 'เหตุผลการปิด (เช่น ความเสี่ยงหมดไป/กระบวนการยกเลิก)' : 'เหตุผลการเปิดใหม่')
    if (!reason || !reason.trim()) return
    const r = await api.riskAct(d.id, d.version, action, reason)
    if (!r.ok) { alert(r.body.error || 'ไม่สำเร็จ'); return }
    d = r.body.risk; msg = action === 'close' ? 'ปิดรายการแล้ว' : 'เปิดใหม่แล้ว'; load()
  }
  async function raisePar() {
    if (!confirm('เปิด PAR สำหรับมาตรการจัดการรายการนี้? (ผู้รับผิดชอบเดียวกัน ติดตามต่อที่เมนู CAR / DAR / PAR)')) return
    busy = true
    const r = await api.riskPar(d.id)
    busy = false
    if (!r.ok) { alert(r.body.error || 'เปิด PAR ไม่สำเร็จ'); return }
    d = r.body.risk; msg = `เปิด ${d.actionRef} แล้ว`; load()
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="shield-alert" size={20} /> ความเสี่ยงและโอกาส</h2>
  <p class="cat-desc">ISO 9001:2015 ข้อ 6.1 — ระบุจากบริบทองค์กร (4.1) และผู้มีส่วนได้ส่วนเสีย (4.2) → ประเมินโอกาสเกิด × ผลกระทบ → แผนจัดการ → ทบทวนประสิทธิผลตามกำหนด</p>
</div>

<div class="grid grid-cols-1 lg:grid-cols-[auto_1fr] gap-4 mb-4">
  <section class="sec !mb-0">
    <h2 class="!mb-2">ผังระดับ (รายการที่ยังติดตาม)</h2>
    <div class="flex gap-2 items-stretch">
      <div class="heat-axis-y">โอกาสเกิด →</div>
      <div>
        <div class="heat">
          {#each [5, 4, 3, 2, 1] as l}
            <div class="heat-lab">{l}</div>
            {#each [1, 2, 3, 4, 5] as i}
              {@const n = heat[l + ':' + i] || 0}
              <button class="heat-cell h-{lvl(l * i)} {cell && cell[0] === l && cell[1] === i ? 'sel' : ''}" title="โอกาสเกิด {l} ({L_TEXT[l]}) × ผลกระทบ {i} ({I_TEXT[i]}) = {l * i}"
                on:click={() => (cell = cell && cell[0] === l && cell[1] === i ? null : [l, i])}>{n || ''}</button>
            {/each}
          {/each}
          <div></div>
          {#each [1, 2, 3, 4, 5] as i}<div class="heat-lab">{i}</div>{/each}
        </div>
        <div class="text-center mut text-[11.5px] mt-1">ผลกระทบ →</div>
      </div>
    </div>
    <div class="flex gap-3 mt-2 text-[11.5px] mut">
      <span><i class="heat-dot h-high"></i> สูง ≥ 15</span><span><i class="heat-dot h-medium"></i> กลาง 8–14</span><span><i class="heat-dot h-low"></i> ต่ำ ≤ 7</span>
    </div>
    {#if cell}<button class="mbtn gh !py-0.5 mt-2" on:click={() => (cell = null)}>ล้างตัวกรองช่อง {cell[0]}×{cell[1]}</button>{/if}
  </section>
  <div class="grid grid-cols-2 gap-3 content-start">
    <div class="card"><div class="n">{openN.filter((r) => r.kind === 'risk').length}</div><div class="l">ความเสี่ยงที่ติดตาม</div></div>
    <div class="card accent"><div class="n">{openN.filter((r) => r.kind === 'opportunity').length}</div><div class="l">โอกาสที่ติดตาม</div></div>
    <div class="card"><div class="n {openN.some((r) => r.level === 'high' && r.kind === 'risk') ? '!text-danger' : ''}">{openN.filter((r) => r.level === 'high' && r.kind === 'risk').length}</div><div class="l">ความเสี่ยงระดับสูง</div></div>
    <div class="card"><div class="n {list.some((r) => r.reviewOverdue) ? '!text-danger' : ''}">{list.filter((r) => r.reviewOverdue).length}</div><div class="l">เลยกำหนดทบทวน</div></div>
  </div>
</div>

<div class="type-tabs">
  <button class="type-tab {kind === 'all' ? 'active' : ''}" on:click={() => (kind = 'all')}>ทั้งหมด <span class="tt-n">{list.length}</span></button>
  <button class="type-tab {kind === 'risk' ? 'active' : ''}" on:click={() => (kind = 'risk')}>ความเสี่ยง <span class="tt-n">{list.filter((r) => r.kind === 'risk').length}</span></button>
  <button class="type-tab {kind === 'opportunity' ? 'active' : ''}" on:click={() => (kind = 'opportunity')}>โอกาส <span class="tt-n">{list.filter((r) => r.kind === 'opportunity').length}</span></button>
  <span class="flex-1"></span>
  {#if me && me.role !== 'viewer'}
    <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => openCreate('risk')}><Icon name="plus" size={15} /> ความเสี่ยง</button>
    <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => openCreate('opportunity')}><Icon name="plus" size={15} /> โอกาส</button>
  {/if}
</div>
<div class="dept-chips">
  {#each FILTERS as [k, label]}<button class="dept-chip {filter === k ? 'active' : ''}" on:click={() => (filter = k)}>{label} <span class="dc-n">{counts[k]}</span></button>{/each}
</div>

{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}

{#if loading}
  <div class="loadwrap"><span class="spin"></span><p class="note m-0">กำลังโหลด…</p></div>
{:else if !shown.length}
  <div class="empty-state"><Icon name="shield-alert" size={38} stroke={1.5} /><p>{list.length ? 'ไม่มีรายการในหมวดนี้' : 'ยังไม่มีทะเบียนความเสี่ยงและโอกาส — เริ่มจากประเด็นภายใน/ภายนอกและความต้องการของผู้มีส่วนได้ส่วนเสีย'}</p></div>
{:else}
  <div class="overflow-x-auto">
    <table class="chg-table">
      <thead><tr><th>เลขที่</th><th>รายการ</th><th>ระดับ</th><th>แนวทาง</th><th>ผู้รับผิดชอบ</th><th>ทบทวนครั้งถัดไป</th><th>สถานะ</th></tr></thead>
      <tbody>
        {#each shown as r (r.id)}
          <tr class="cursor-pointer hover:bg-brand-light/40" on:click={() => openDetail(r)}>
            <td class="nowrap font-semibold">{r.refNo}</td>
            <td><div class="font-semibold">{r.title}</div><div class="mut text-[12px]">{KIND[r.kind]}{r.context ? ' · ' + CONTEXT[r.context] : ''}{r.dept ? ' · ' + (DEPTNAME[r.dept] || r.dept) : ''}{r.process ? ' · ' + r.process : ''}</div></td>
            <td class="nowrap"><span class="lvl-badge h-{r.level}">{LEVEL[r.level]} {r.score}</span>{#if r.residualScore}<div class="mut text-[11.5px]">คงเหลือ {LEVEL[r.residualLevel]} {r.residualScore}</div>{/if}</td>
            <td>{TREAT[r.kind][r.treatment] || '—'}{#if r.actionRef}<div class="mut text-[11.5px]">{r.actionRef}</div>{/if}</td>
            <td>{r.ownerName || '—'}</td>
            <td class="nowrap {r.reviewOverdue ? 'text-danger font-bold' : ''}">{r.reviewDue || '—'}</td>
            <td class="nowrap">
              {#if r.status === 'closed'}<span class="status-badge s-closed">ปิดแล้ว</span>
              {:else if r.reviewOverdue}<span class="status-badge s-overdue">เลยกำหนดทบทวน</span>
              {:else if r.gaps.length}<span class="status-badge s-in_progress" title={'ยังขาด: ' + r.gaps.join(', ')}>ข้อมูลไม่ครบ</span>
              {:else}<span class="status-badge s-open">{STATUS[r.status]}</span>{/if}
            </td>
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
        <div class="mt"><b>{d.refNo}</b> {d.title}
          <div class="msub">{KIND[d.kind]} · บันทึกโดย {d.createdName} · {fmtWhen(d.createdAt)}</div>
        </div>
        <button class="mx inline-flex items-center justify-center" on:click={() => (d = null)}><Icon name="x" size={16} /></button>
      </header>
      <div class="mbody">
        {#if msg}<div class="save-ok">{msg}</div>{/if}
        {#if d.status === 'closed'}<div class="merr">ปิดแล้ว: {d.closeReason}</div>{/if}
        {#if d.gaps.length && d.status !== 'closed'}<div class="merr">ยังขาด: {d.gaps.join(', ')}</div>{/if}
        <div class="mr-bar">
          {#if d.can.review}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={startReview}><Icon name="refresh-cw" size={14} /> ทบทวน</button>{/if}
          {#if d.can.edit}<button class="mbtn gh" on:click={openEdit}>แก้ไข</button>{/if}
          {#if d.can.par}<button class="mbtn gh" disabled={busy} on:click={raisePar}>เปิด PAR</button>{/if}
          {#if d.actionRef}<button class="mbtn gh" on:click={() => dispatch('goto', { tab: 'act' })}>{d.actionRef}</button>{/if}
          {#if d.can.close}<button class="mbtn gh" on:click={() => act('close')}>ปิดรายการ</button>{/if}
          {#if d.can.reopen}<button class="mbtn gh" on:click={() => act('reopen')}>เปิดใหม่</button>{/if}
          <span class="mnote">ทบทวนครั้งถัดไป <b class={d.reviewOverdue ? 'text-danger' : ''}>{d.reviewDue || '—'}</b>{#if d.lastReviewAt} · ล่าสุด {fmtWhen(d.lastReviewAt)} โดย {d.lastReviewBy}{/if}</span>
        </div>

        {#if rv}
          <section class="sec">
            <h2 class="!mb-2">ทบทวนและประเมินซ้ำ (6.1.2 b)</h2>
            <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
              <div class="sgf"><label>โอกาสเกิด</label><select bind:value={rv.likelihood}>{#each [1, 2, 3, 4, 5] as n}<option value={n}>{n} · {L_TEXT[n]}</option>{/each}</select></div>
              <div class="sgf"><label>ผลกระทบ</label><select bind:value={rv.impact}>{#each [1, 2, 3, 4, 5] as n}<option value={n}>{n} · {I_TEXT[n]}</option>{/each}</select></div>
              <div class="sgf"><label>ประสิทธิผลของการจัดการ</label><select bind:value={rv.effectiveness}>{#each Object.entries(EFF) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
              <div class="sgf"><label>ทบทวนครั้งถัดไป</label><input type="date" min={plusDays(1)} bind:value={rv.nextReview} /></div>
            </div>
            <p class="mnote">คะแนนใหม่ {rv.likelihood * rv.impact} ({LEVEL[lvl(rv.likelihood * rv.impact)]}) เดิม {d.score} ({LEVEL[d.level]})</p>
            <div class="sgf"><label>ผลการทบทวน / หลักฐาน <em>*</em></label><textarea rows="2" bind:value={rv.note} placeholder="มาตรการดำเนินการแล้วหรือยัง เกิดเหตุการณ์หรือไม่ ข้อมูลที่ใช้ประเมิน"></textarea></div>
            {#if rvErr}<div class="merr">{rvErr}</div>{/if}
            <div class="sgact !justify-start"><button class="mbtn accent" disabled={busy} on:click={saveReview}>{busy ? 'กำลังบันทึก…' : 'บันทึกการทบทวน'}</button><button class="mbtn gh" on:click={() => (rv = null)}>ยกเลิก</button></div>
          </section>
        {/if}

        <div class="capa-grid">
          <section class="sec">
            <h2 class="!mb-2">ที่มาและการประเมิน</h2>
            <div class="det-info">
              <div><b>ที่มา:</b> {CONTEXT[d.context] || '—'}</div><div><b>ผู้มีส่วนได้ส่วนเสีย:</b> {d.party || '—'}</div>
              <div><b>กระบวนการ:</b> {d.process || '—'}</div><div><b>แผนก:</b> {d.dept ? DEPTNAME[d.dept] || d.dept : '—'}</div>
              <div class="col-span-2"><b>รายละเอียด:</b> <span class="whitespace-pre-wrap">{d.detail || '—'}</span></div>
              <div><b>โอกาสเกิด × ผลกระทบ:</b> {d.likelihood} × {d.impact} = <span class="lvl-badge h-{d.level}">{LEVEL[d.level]} {d.score}</span></div>
              <div><b>มาตรการที่มีอยู่:</b> {d.existingControls || '—'}</div>
            </div>
          </section>
          <section class="sec">
            <h2 class="!mb-2">การจัดการ (6.1.2 a)</h2>
            <div class="det-info">
              <div><b>แนวทาง:</b> {TREAT[d.kind][d.treatment] || '—'}</div><div><b>ผู้รับผิดชอบ:</b> {d.ownerName || '—'}</div>
              <div class="col-span-2"><b>{d.treatment === 'accept' ? 'เหตุผลการยอมรับ' : 'แผน/มาตรการ'}:</b> <span class="whitespace-pre-wrap">{d.plan || '—'}</span></div>
              <div><b>ระดับคงเหลือเป้าหมาย:</b> {#if d.residualScore}<span class="lvl-badge h-{d.residualLevel}">{LEVEL[d.residualLevel]} {d.residualScore}</span>{:else}—{/if}</div>
              <div><b>ประสิทธิผล:</b> {EFF[d.effectiveness]}</div>
            </div>
          </section>
        </div>

        {#if d.history?.length}
          <h3 class="rd-sub">ประวัติ</h3>
          <table class="chg-table"><thead><tr><th>เมื่อ</th><th>ผู้ดำเนินการ</th><th>การดำเนินการ</th><th>คะแนน</th><th>รายละเอียด</th></tr></thead>
            <tbody>{#each [...d.history].reverse() as h}<tr><td class="nowrap">{fmtWhen(h.at)}</td><td>{h.actorName}</td><td>{EVENT[h.event] || h.event}{#if h.effectiveness} · {EFF[h.effectiveness]}{/if}</td><td class="nowrap">{h.likelihood}×{h.impact}={h.likelihood * h.impact}</td><td>{h.note || '—'}</td></tr>{/each}</tbody></table>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if f}
  <div class="sgbg" on:mousedown|self={() => (f = null)}>
    <div class="sgbox wide">
      <h3>{editing ? `แก้ไข ${editing.refNo}` : `บันทึก${KIND[f.kind]}ใหม่`}</h3>
      <div class="sgf"><label>{KIND[f.kind]} <em>*</em></label><input bind:value={f.title} placeholder={f.kind === 'risk' ? 'เช่น วัตถุดิบเหล็กขาดตลาด ส่งมอบล่าช้า' : 'เช่น ลูกค้ากลุ่มโครงการหมู่บ้านขยายตัว'} /></div>
      <div class="sgf"><label>รายละเอียด / สาเหตุ / ผลที่อาจเกิด</label><textarea rows="2" bind:value={f.detail}></textarea></div>
      <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
        <div class="sgf"><label>ที่มา</label><select bind:value={f.context}><option value="">—</option>{#each Object.entries(CONTEXT) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
        <div class="sgf"><label>ผู้มีส่วนได้ส่วนเสีย</label><input bind:value={f.party} placeholder="ลูกค้า, ผู้ขาย, ชุมชน…" /></div>
        <div class="sgf"><label>กระบวนการ</label><input bind:value={f.process} /></div>
        <div class="sgf"><label>แผนก</label><select bind:value={f.dept}><option value="">—</option>{#each DEPTS as dp}<option value={dp}>{DEPTNAME[dp]}</option>{/each}</select></div>
      </div>
      <div class="grid grid-cols-2 md:grid-cols-3 gap-3">
        <div class="sgf"><label>โอกาสเกิด</label><select bind:value={f.likelihood}>{#each [1, 2, 3, 4, 5] as n}<option value={n}>{n} · {L_TEXT[n]}</option>{/each}</select></div>
        <div class="sgf"><label>{f.kind === 'risk' ? 'ผลกระทบ' : 'ประโยชน์'}</label><select bind:value={f.impact}>{#each [1, 2, 3, 4, 5] as n}<option value={n}>{n} · {I_TEXT[n]}</option>{/each}</select></div>
        <div class="sgf"><label>ระดับ</label><div class="pt-1.5"><span class="lvl-badge h-{lvl(fScore)}">{LEVEL[lvl(fScore)]} {fScore}</span></div></div>
      </div>
      <div class="sgf"><label>มาตรการที่มีอยู่แล้ว</label><input bind:value={f.existingControls} placeholder="เช่น มีผู้ขายสำรอง 2 ราย, ตรวจรับ 100%" /></div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>แนวทางจัดการ <em>*</em></label><select bind:value={f.treatment}><option value="">—</option>{#each Object.entries(TREAT[f.kind]) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
        <div class="sgf mr-person"><label>ผู้รับผิดชอบ <em>*</em></label>
          <input bind:value={pq} on:input={findPeople} placeholder="พิมพ์ชื่อหรือรหัสพนักงาน" />
          {#if f.ownerCode}<div class="mut text-[12px]">เลือก: {f.ownerName} ({f.ownerCode})</div>{/if}
          {#if people.length}<div class="mr-people">{#each people as p}<button on:click={() => pickPerson(p)}>{p.empCode} · {p.nameTh} <span class="mut">{p.position}</span></button>{/each}</div>{/if}
        </div>
      </div>
      <div class="sgf"><label>{f.treatment === 'accept' ? 'เหตุผลการยอมรับ' : 'แผนจัดการ / มาตรการ'}{lvl(fScore) !== 'low' && f.treatment !== 'accept' ? ' *' : ''}</label><textarea rows="3" bind:value={f.plan} placeholder="ทำอะไร ใคร เมื่อไร — มาตรการต้องเหมาะสมกับผลกระทบต่อความสอดคล้องของผลิตภัณฑ์ (6.1.2)"></textarea></div>
      <div class="grid grid-cols-2 md:grid-cols-3 gap-3">
        <div class="sgf"><label>โอกาสเกิดหลังจัดการ (เป้าหมาย)</label><select bind:value={f.residualLikelihood}><option value={0}>—</option>{#each [1, 2, 3, 4, 5] as n}<option value={n}>{n}</option>{/each}</select></div>
        <div class="sgf"><label>ผลกระทบหลังจัดการ (เป้าหมาย)</label><select bind:value={f.residualImpact}><option value={0}>—</option>{#each [1, 2, 3, 4, 5] as n}<option value={n}>{n}</option>{/each}</select></div>
        <div class="sgf"><label>กำหนดทบทวน</label><input type="date" min={today()} bind:value={f.reviewDue} />{#if !editing && !f.reviewDue}<div class="mut text-[11.5px]">ว่าง = อัตโนมัติ ({lvl(fScore) === 'high' ? '3 เดือน' : '12 เดือน'})</div>{/if}</div>
      </div>
      {#if fResidual}<p class="mnote">ระดับคงเหลือเป้าหมาย: <span class="lvl-badge h-{lvl(fResidual)}">{LEVEL[lvl(fResidual)]} {fResidual}</span></p>{/if}
      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact"><button class="mbtn gh" on:click={() => (f = null)}>ยกเลิก</button><button class="mbtn accent" disabled={saving} on:click={saveForm}>{saving ? 'กำลังบันทึก…' : 'บันทึก'}</button></div>
    </div>
  </div>
{/if}
