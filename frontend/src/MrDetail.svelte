<script>
  // การประชุมทบทวนฝ่ายบริหารหนึ่งรายการ: ภาพรวม · ข้อมูลนำเข้า · มติและงาน · ลงนามและบันทึกการประชุม
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { MR_STATUS, MR_STEPS, HEALTH, SIG_STAGE, SIG_ACTION } from './lib/mreview.js'
  import { fmtDate, fmtWhen } from './lib/changeNote.js'
  import MrInputs from './MrInputs.svelte'
  import MrActions from './MrActions.svelte'
  import MrMinutes from './MrMinutes.svelte'
  import DecisionDialog from './DecisionDialog.svelte'
  import Icon from './Icon.svelte'

  export let id
  export let me = null
  const dispatch = createEventDispatcher()

  let m = null, err = '', msg = '', tab = 'overview', topic = null
  let deciding = null // submit | pass | return | approve | close | revise
  let editing = false, form = {}, formErr = ''

  async function load() {
    err = ''
    try { m = await api.mrGet(id); if (m.error) throw new Error(m.error) } catch (e) { err = 'โหลดไม่สำเร็จ: ' + e.message }
  }
  onMount(load)
  /** ผลจาก API ที่บันทึกสำเร็จ → ใช้แทนข้อมูลเดิม; error → แสดง */
  function apply(r, okMsg = '') {
    if (!r.ok) { err = r.body.error || 'บันทึกไม่สำเร็จ'; if (r.status === 409) load(); return false }
    m = r.body
    err = ''
    msg = okMsg
    if (okMsg) setTimeout(() => { if (msg === okMsg) msg = '' }, 5000)
    return true
  }
  async function sync() { apply(await api.mrSync(m.id, m.version), 'ดึงข้อมูลจากระบบล่าสุดแล้ว') }

  $: can = m?.can || {}
  $: stepIdx = m ? MR_STEPS.indexOf(m.status) : -1
  $: comp = m?.completeness

  const DECIDE = {
    submit: { title: 'ลงนามผู้จัดทำ — ส่งตรวจสอบ', label: 'ส่งตรวจสอบ', comment: 'หมายเหตุ' },
    pass: { title: 'ลงนามผู้ตรวจสอบ — ส่งผู้อนุมัติ', label: 'ตรวจสอบผ่าน', comment: 'ความเห็นผู้ตรวจสอบ' },
    return: { title: 'ตีกลับเป็นร่าง', label: 'ตีกลับ', comment: 'เหตุผลที่ตีกลับ', required: true, danger: true },
    approve: { title: 'ลงนามผู้อนุมัติ — อนุมัติมติและงานติดตาม', label: 'อนุมัติ', comment: 'ความเห็นฝ่ายบริหาร' },
    close: { title: 'ปิดการประชุมทบทวน', label: 'ปิดการประชุม', comment: 'สรุปผลการติดตาม' },
    revise: { title: 'สร้างฉบับแก้ไข (revision)', label: 'สร้างฉบับแก้ไข', comment: 'เหตุผลการแก้ไข', required: true, noPin: true },
  }
  $: dd = deciding ? DECIDE[deciding] : null
  async function onDecide(e) {
    const { comment, pin, fail } = e.detail
    const r = deciding === 'revise' ? await api.mrRevise(m.id, m.version, comment) : await api.mrAct(m.id, m.version, deciding, comment, pin)
    if (!r.ok) { fail(r.body.error || 'บันทึกไม่สำเร็จ'); return }
    const was = deciding
    deciding = null
    if (was === 'revise') { dispatch('open', r.body.id); return }
    apply(r, { submit: 'ส่งตรวจสอบแล้ว', pass: 'ตรวจสอบผ่าน — ส่งผู้อนุมัติแล้ว', return: 'ตีกลับเป็นร่างแล้ว', approve: 'อนุมัติแล้ว — เริ่มติดตามงาน', close: 'ปิดการประชุมแล้ว' }[was])
  }

  function startEdit() {
    form = { period: m.period, meetDate: m.meetDate, meetTime: m.meetTime, location: m.location, chair: m.chair, secretary: m.secretary, attendees: m.attendees, nextDue: m.nextDue }
    formErr = ''
    editing = true
  }
  async function saveFields() {
    const r = await api.mrUpdate(m.id, m.version, form)
    if (!r.ok) { formErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    editing = false
    apply(r, 'บันทึกรายละเอียดการประชุมแล้ว')
  }
  function goCheck(c) {
    if (m.topics.some((t) => t.key === c.key)) { topic = c.key; tab = 'inputs' }
    else if (c.key === 'decisions') { topic = m.inputs.find((i) => !i.decision.trim())?.key || 'previous'; tab = 'inputs' }
    else tab = 'actions'
  }
  function goInsight(x) {
    if (x.topic === 'actions') tab = 'actions'
    else { topic = x.topic; tab = 'inputs' }
  }
  const topicTitle = (k) => m?.topics.find((t) => t.key === k)?.title || k
</script>

<div class="no-print flex flex-wrap items-center gap-2 mb-3">
  <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => dispatch('back')}><Icon name="chevrons-left" size={14} /> รายการประชุม</button>
  {#if m}
    <b class="text-brand text-[16px]">{m.refNo}</b>
    {#if m.revision}<span class="rd-cat">ฉบับแก้ไข {m.revision}</span>{/if}
    <span class="status-badge mr-{m.status}">{MR_STATUS[m.status]}</span>
    {#if (m.revisions || []).length > 1}
      {#each m.revisions as r}
        {#if r.id !== m.id}<button class="rd-chip doc" on:click={() => dispatch('open', r.id)}>ฉบับ {r.revision} · {MR_STATUS[r.status]}</button>{/if}
      {/each}
    {/if}
  {/if}
</div>

{#if err}<div class="merr no-print"><span>{err}</span><button class="retry" on:click={load}>โหลดใหม่</button></div>{/if}
{#if msg}<div class="save-ok no-print">{msg}</div>{/if}

{#if !m}
  {#if !err}<div class="loadwrap"><span class="spin"></span></div>{/if}
{:else}
  <!-- ขั้นตอน -->
  <div class="mr-steps no-print">
    {#each MR_STEPS as st, i}
      <div class="mr-step" class:done={stepIdx > i || m.status === 'closed'} class:cur={stepIdx === i && m.status !== 'closed'}>
        <span class="mr-step-n">{#if stepIdx > i || m.status === 'closed'}<Icon name="circle-check" size={14} />{:else}{i + 1}{/if}</span>{MR_STATUS[st]}
      </div>
    {/each}
    {#if m.status === 'superseded'}<div class="mr-step cur">ถูกแทนที่ด้วยฉบับแก้ไข</div>{/if}
  </div>

  <div class="mr-bar no-print">
    {#if can.edit}<button class="mbtn inline-flex items-center gap-1.5" on:click={sync}><Icon name="refresh-cw" size={14} /> ดึงข้อมูลจากระบบล่าสุด</button>{/if}
    {#if m.status === 'draft' && me?.role !== 'viewer'}
      <button class="mbtn accent inline-flex items-center gap-1.5" disabled={!can.submit} title={can.submit ? '' : 'ต้องครบ 100% ก่อน'} on:click={() => (deciding = 'submit')}><Icon name="pen-line" size={14} /> ลงนามผู้จัดทำ · ส่งตรวจสอบ ({can.percent}%)</button>
    {/if}
    {#if can.review}
      <button class="mbtn accent" on:click={() => (deciding = 'pass')}>ตรวจสอบผ่าน</button>
      <button class="mbtn danger" on:click={() => (deciding = 'return')}>ตีกลับ</button>
    {/if}
    {#if can.approve}
      <button class="mbtn accent" on:click={() => (deciding = 'approve')}>อนุมัติ</button>
      <button class="mbtn danger" on:click={() => (deciding = 'return')}>ตีกลับ</button>
    {/if}
    {#if can.close}<button class="mbtn accent" on:click={() => (deciding = 'close')}>ปิดการประชุม</button>{/if}
    {#if can.revise}<button class="mbtn gh" on:click={() => (deciding = 'revise')}>สร้างฉบับแก้ไข</button>{/if}
    {#if (m.status === 'review' && !can.review) || (m.status === 'approve' && !can.approve)}
      <span class="mnote">รอ{m.status === 'review' ? 'ผู้ตรวจสอบ' : 'ผู้อนุมัติ'}ลงนาม — ผู้จัดทำ ผู้ตรวจสอบ และผู้อนุมัติต้องเป็นคนละคน</span>
    {/if}
    {#if m.status === 'monitoring' && !can.close && (me?.role === 'approver' || me?.role === 'admin')}
      <span class="mnote">ปิดได้เมื่อทุกงานเสร็จและทวนสอบแล้ว (ค้าง {can.openActions} · รอทวนสอบ {can.unverified})</span>
    {/if}
  </div>

  <nav class="mtabs no-print !mb-3">
    <button class={tab === 'overview' ? 'active' : ''} on:click={() => (tab = 'overview')}>ภาพรวม</button>
    <button class={tab === 'inputs' ? 'active' : ''} on:click={() => (tab = 'inputs')}>ข้อมูลนำเข้า ({comp.reviewed}/13)</button>
    <button class={tab === 'actions' ? 'active' : ''} on:click={() => (tab = 'actions')}>มติและงานติดตาม ({m.actions.length})</button>
    <button class={tab === 'minutes' ? 'active' : ''} on:click={() => (tab = 'minutes')}>ลงนามและบันทึกการประชุม</button>
  </nav>

  {#if tab === 'overview'}
    <div class="rd-metrics">
      <div class="rd-metric"><div class="rd-ml">ความครบถ้วน<Icon name="list-checks" size={18} /></div><div class="rd-mv">{comp.percent}%</div><div class="rd-mn"><i class="dot {comp.percent === 100 ? 'ok' : 'warn'}"></i>{comp.passed}/{comp.total} รายการ</div></div>
      <div class="rd-metric"><div class="rd-ml">ข้อมูลนำเข้าที่ทบทวนแล้ว<Icon name="clipboard-list" size={18} /></div><div class="rd-mv">{comp.reviewed}/13</div><div class="rd-mn"><i class="dot {comp.reviewed === 13 ? 'ok' : 'warn'}"></i>ตาม 9.3.2 a–f</div></div>
      <div class="rd-metric"><div class="rd-ml">งานติดตาม<Icon name="flag" size={18} /></div><div class="rd-mv">{m.actions.length}</div><div class="rd-mn"><i class="dot {can.openActions ? 'warn' : 'ok'}"></i>ค้าง {can.openActions} · รอทวนสอบ {can.unverified}</div></div>
      <div class="rd-metric"><div class="rd-ml">ประเด็นที่ควรพิจารณา<Icon name="alert-triangle" size={18} /></div><div class="rd-mv">{m.insights.filter((x) => x.level === 'critical' || x.level === 'attention').length}</div><div class="rd-mn"><i class="dot {m.insights.some((x) => x.level === 'critical') ? 'bad' : 'warn'}"></i>จากข้อมูลนำเข้าและงาน</div></div>
    </div>

    <div class="grid gap-4 mr-two">
      <div class="sec">
        <div class="flex items-center gap-2 mb-2"><h2 class="!m-0">รายละเอียดการประชุม</h2><span class="flex-1"></span>{#if can.edit}<button class="mbtn gh" on:click={startEdit}>แก้ไข</button>{/if}</div>
        <div class="det-info">
          <div><b>รอบ:</b> {m.period || '—'}</div><div><b>วันที่:</b> {fmtDate(m.meetDate)} {m.meetTime}</div>
          <div><b>สถานที่:</b> {m.location || '—'}</div><div><b>ประธาน:</b> {m.chair || '—'}</div>
          <div><b>เลขานุการ:</b> {m.secretary || '—'}</div><div><b>ทบทวนครั้งถัดไป:</b> {m.nextDue ? fmtDate(m.nextDue) : '—'}</div>
          <div><b>สร้างโดย:</b> {m.createdName}</div><div><b>ดึงข้อมูลระบบล่าสุด:</b> {m.syncedAt ? fmtWhen(m.syncedAt) : '—'}</div>
        </div>
        {#if m.attendees}<div class="text-[12.5px] mt-2"><b class="text-mut">ผู้เข้าประชุม:</b><div class="whitespace-pre-line">{m.attendees}</div></div>{/if}
      </div>
      <div class="sec">
        <h2 class="!mb-2">รายการความครบถ้วน</h2>
        {#each comp.checks as c}
          <button class="rd-attn-row" on:click={() => goCheck(c)}>
            <span class="rd-dot {c.pass ? 's-pass' : 's-warning'}"></span><b>{c.title}</b><span class="mut truncate">{c.reason}</span>
          </button>
        {/each}
      </div>
    </div>

    {#if m.insights.length}
      <div class="sec mt-4">
        <h2 class="!mb-2">ประเด็นที่ควรหยิบยกในที่ประชุม</h2>
        {#each m.insights.slice(0, 30) as x}
          <button class="rd-attn-row" on:click={() => goInsight(x)}>
            <span class="rd-badge s-{x.level === 'critical' ? 'critical' : x.level === 'good' ? 'pass' : 'warning'}">{HEALTH[x.level] || x.level}</span>
            <span class="rd-clause">{x.topic === 'actions' ? 'งานติดตาม' : topicTitle(x.topic)}</span>
            <span class="truncate">{x.text}</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if m.signatures.length}
      <div class="sec mt-4">
        <h2 class="!mb-2">ประวัติการลงนาม</h2>
        <table class="chg-table">
          <thead><tr><th>ขั้น</th><th>การดำเนินการ</th><th>ผู้ลงนาม</th><th>วันเวลา</th><th>ความเห็น</th></tr></thead>
          <tbody>{#each m.signatures as g}<tr><td>{SIG_STAGE[g.stage] || g.stage}</td><td>{SIG_ACTION[g.action] || g.action}</td><td>{g.name}<br /><span class="mut">{g.position}</span></td><td class="nowrap">{fmtWhen(g.at)}</td><td>{g.comment || '—'}</td></tr>{/each}</tbody>
        </table>
      </div>
    {/if}
  {:else if tab === 'inputs'}
    <MrInputs {m} {me} bind:topic on:saved={(e) => apply(e.detail, 'บันทึกหัวข้อแล้ว')} on:doc />
  {:else if tab === 'actions'}
    <MrActions {m} {me} on:saved={(e) => apply(e.detail, 'บันทึกงานแล้ว')} />
  {:else}
    <MrMinutes {m} />
  {/if}
{/if}

{#if dd}
  <DecisionDialog
    title={dd.title}
    message="{m.refNo} · ทบทวนฝ่ายบริหาร {m.period}"
    {me}
    commentLabel={dd.comment}
    commentRequired={!!dd.required}
    withPin={!dd.noPin}
    confirmLabel={dd.label}
    danger={!!dd.danger}
    on:confirm={onDecide}
    on:cancel={() => (deciding = null)}
  />
{/if}

{#if editing}
  <div class="sgbg" on:mousedown|self={() => (editing = false)}>
    <div class="sgbox wide">
      <h3>รายละเอียดการประชุม</h3>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>รอบการทบทวน</label><input bind:value={form.period} /></div>
        <div class="sgf"><label>วันที่ประชุม <em>*</em></label><input type="date" bind:value={form.meetDate} /></div>
        <div class="sgf"><label>เวลา</label><input bind:value={form.meetTime} /></div>
        <div class="sgf"><label>สถานที่</label><input bind:value={form.location} /></div>
        <div class="sgf"><label>ประธาน</label><input bind:value={form.chair} /></div>
        <div class="sgf"><label>เลขานุการ</label><input bind:value={form.secretary} /></div>
      </div>
      <div class="sgf"><label>ผู้เข้าประชุม</label><textarea rows="3" bind:value={form.attendees}></textarea></div>
      <div class="sgf"><label>กำหนดทบทวนครั้งถัดไป</label><input type="date" bind:value={form.nextDue} /></div>
      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact"><button class="mbtn gh" on:click={() => (editing = false)}>ยกเลิก</button><button class="mbtn accent" on:click={saveFields}>บันทึก</button></div>
    </div>
  </div>
{/if}
