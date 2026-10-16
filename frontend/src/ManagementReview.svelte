<script>
  // ทบทวนฝ่ายบริหาร (ISO 9001:2015 ข้อ 9.3) — รายการประชุม งานที่ฉันรับผิดชอบ และเปิดรายละเอียด
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { MR_STATUS, today } from './lib/mreview.js'
  import { fmtDate } from './lib/changeNote.js'
  import MrDetail from './MrDetail.svelte'
  import Icon from './Icon.svelte'

  export let me = null
  const dispatch = createEventDispatcher()

  let data = null, err = '', openId = null
  let creating = false, form = {}, formErr = '', saving = false

  async function load() {
    err = ''
    try { data = await api.mrList() } catch (e) { err = 'โหลดรายการไม่สำเร็จ: ' + e.message }
  }
  onMount(load)

  function newForm() {
    const y = new Date().getFullYear() + 543
    const q = Math.floor(new Date().getMonth() / 3) + 1
    form = { period: `Q${q}/${y}`, meetDate: today(), meetTime: '09:00', location: '', chair: '', secretary: me?.name || '', attendees: '', nextDue: '' }
    formErr = ''
    creating = true
  }
  async function create() {
    saving = true
    formErr = ''
    const r = await api.mrCreate(form)
    saving = false
    if (!r.ok) { formErr = r.body.error || 'สร้างไม่สำเร็จ'; return }
    creating = false
    openId = r.body.id
    load()
  }
</script>

{#if openId}
  {#key openId}
    <MrDetail id={openId} {me} on:back={() => { openId = null; load() }} on:open={(e) => (openId = e.detail)} on:doc />
  {/key}
{:else}
  <div class="cat-head">
    <h2 class="cat-title inline-flex items-center gap-2"><Icon name="building" size={20} /> ทบทวนฝ่ายบริหาร</h2>
    <p class="cat-desc">ISO 9001:2015 ข้อ 9.3 — ข้อมูลนำเข้า 13 หัวข้อดึงจากระบบจริง · มติและงานติดตาม · ลงนามผู้จัดทำ → ผู้ตรวจสอบ → ผู้อนุมัติ ด้วยบัญชีพนักงานและ PIN</p>
  </div>

  {#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}
  {#if !data}
    <div class="loadwrap"><span class="spin"></span></div>
  {:else}
    {#if data.myActions.length}
      <div class="sec mb-4">
        <h2 class="!mb-2">งานที่ฉันรับผิดชอบ ({data.myActions.length})</h2>
        <table class="chg-table">
          <thead><tr><th>เลขที่</th><th>งาน</th><th>กำหนดเสร็จ</th><th>สถานะ</th></tr></thead>
          <tbody>
            {#each data.myActions as a}
              <tr class="clickrow" on:click={() => (openId = a.meetingId)}>
                <td class="code">{a.refNo}</td><td>{a.title}</td>
                <td class="nowrap" class:text-danger={a.dueDate && a.dueDate < today()}>{a.dueDate || '—'}</td>
                <td>{a.status === 'in_progress' ? 'กำลังดำเนินการ' : 'ยังไม่เริ่ม'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    <div class="flex items-center gap-2 mb-3">
      <b class="text-brand">การประชุมทั้งหมด ({data.meetings.length})</b>
      <span class="flex-1"></span>
      {#if data.canCreate}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={newForm}><Icon name="plus" size={14} /> สร้างการประชุมทบทวน</button>{/if}
    </div>

    {#if !data.meetings.length}
      <div class="empty-state"><Icon name="building" size={34} stroke={1.5} /><p>ยังไม่มีการประชุมทบทวนฝ่ายบริหารในระบบ</p></div>
    {:else}
      <div class="act-grid">
        {#each data.meetings as m (m.id)}
          <div class="act-card" role="button" tabindex="0" on:click={() => (openId = m.id)} on:keydown={(e) => e.key === 'Enter' && (openId = m.id)}>
            <div class="ac-top">
              <span class="ac-ref">{m.refNo}{#if m.revision} · ฉบับแก้ไข {m.revision}{/if}</span>
              <span class="flex-1"></span>
              <span class="status-badge mr-{m.status}">{MR_STATUS[m.status]}</span>
            </div>
            <div class="ac-title">ทบทวนฝ่ายบริหาร {m.period}</div>
            <div class="ac-meta">
              <span>ประชุม {fmtDate(m.meetDate)}</span>
              {#if m.chair}<span>ประธาน {m.chair}</span>{/if}
              <span>งาน {m.actions}{#if m.openActions} · ค้าง {m.openActions}{/if}{#if m.lateActions} · <b class="text-danger">เลยกำหนด {m.lateActions}</b>{/if}</span>
            </div>
            <div class="prog"><div class="prog-fill" style="width:{m.percent}%"></div></div>
            <div class="ac-prog-n">ความครบถ้วน {m.percent}%</div>
          </div>
        {/each}
      </div>
    {/if}
  {/if}

  {#if creating}
    <div class="sgbg" on:mousedown|self={() => (creating = false)}>
      <div class="sgbox wide">
        <h3>สร้างการประชุมทบทวนฝ่ายบริหาร</h3>
        <div class="sgs">ระบบจะดึงข้อมูลนำเข้าจากระบบให้ทันที (DCR, CAR/DAR/PAR, ผลความพร้อม, การอบรม, งานจากครั้งก่อน)</div>
        <div class="grid grid-cols-2 gap-3">
          <div class="sgf"><label>รอบการทบทวน <em>*</em></label><input bind:value={form.period} placeholder="เช่น Q3/2569" /></div>
          <div class="sgf"><label>วันที่ประชุม <em>*</em></label><input type="date" bind:value={form.meetDate} /></div>
          <div class="sgf"><label>เวลา</label><input bind:value={form.meetTime} /></div>
          <div class="sgf"><label>สถานที่</label><input bind:value={form.location} /></div>
          <div class="sgf"><label>ประธาน</label><input bind:value={form.chair} /></div>
          <div class="sgf"><label>เลขานุการ</label><input bind:value={form.secretary} /></div>
        </div>
        <div class="sgf"><label>ผู้เข้าประชุม</label><textarea rows="2" bind:value={form.attendees} placeholder="หนึ่งคนต่อบรรทัด: ชื่อ – ตำแหน่ง"></textarea></div>
        <div class="sgf"><label>กำหนดทบทวนครั้งถัดไป</label><input type="date" bind:value={form.nextDue} /></div>
        {#if formErr}<div class="merr">{formErr}</div>{/if}
        <div class="sgact">
          <button class="mbtn gh" on:click={() => (creating = false)}>ยกเลิก</button>
          <button class="mbtn accent" disabled={saving} on:click={create}>{saving ? 'กำลังสร้าง…' : 'สร้างและดึงข้อมูล'}</button>
        </div>
      </div>
    </div>
  {/if}
{/if}
