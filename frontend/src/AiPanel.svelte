<script>
  // ผู้ช่วย AI — แสดงข้อแนะนำทีละข้อ ให้ผู้ใช้เลือก "ทำตาม" หรือ "ไม่ทำตาม" เอง (ระบบไม่แก้อะไรจนกว่าผู้ใช้สั่ง)
  //   kind 'doc': ตรวจเอกสาร → ข้อที่ทำตามและมีข้อความเดิม/ใหม่ นำไปสร้างฉบับร่างส่ง DCR ได้ (event apply)
  //   kind 'dcr': ช่วยผู้ตรวจคำขอเปลี่ยนแปลง → ข้อที่ทำตามส่งออกทาง bind:accepted ให้ใส่ในความเห็นผู้ตรวจ
  import { createEventDispatcher, onMount } from 'svelte'
  import { api } from './lib/api.js'
  import { fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let kind = 'doc'
  export let subject = ''
  export let me = null
  /** สร้างฉบับร่างจากข้อที่ทำตามได้ (เอกสารฉบับปัจจุบัน .docx/.xlsx ที่ยังไม่มีคำขอค้าง) */
  export let canApply = false
  /** เหตุผลที่สร้างฉบับร่างอัตโนมัติไม่ได้ (เช่น มีคำขอค้างอยู่) */
  export let blockedReason = ''
  export let accepted = []
  const dispatch = createEventDispatcher()

  const SEV = { high: ['สำคัญมาก', 'r-major'], medium: ['ควรแก้', 'r-minor'], low: ['เล็กน้อย', ''] }
  const CAT = { rev: 'เลขฉบับ/วันที่', reference: 'การอ้างอิงเอกสาร', iso: 'ข้อกำหนด ISO', consistency: 'ความสอดคล้อง', clarity: 'ความชัดเจน', missing: 'ขาดหัวข้อ/ช่อง', format: 'รูปแบบ' }

  let status = null, review = null, busy = false, applying = false, err = '', loading = true
  $: list = review?.suggestions || []
  $: accepted = list.filter((s) => s.status === 'accepted')
  $: rejected = list.filter((s) => s.status === 'rejected').length
  $: autoFix = accepted.filter((s) => s.find)
  $: readonly = me?.role === 'viewer'

  onMount(async () => {
    try {
      status = await api.aiStatus()
      review = await api.aiLatest(kind, String(subject))
    } catch (e) { err = 'โหลดผู้ช่วย AI ไม่สำเร็จ: ' + e.message }
    loading = false
  })

  async function run() {
    busy = true; err = ''
    const r = kind === 'doc' ? await api.aiReviewDoc(subject) : await api.aiReviewChange(subject)
    busy = false
    if (!r.ok) { err = r.body.error || 'ผู้ช่วย AI ทำงานไม่สำเร็จ'; return }
    review = r.body
  }

  async function decide(s, decision) {
    err = ''
    const next = s.status === decision ? 'open' : decision // กดซ้ำ = ยกเลิกการตัดสินใจ
    const r = await api.aiDecide(s.id, next)
    if (!r.ok) { err = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    review = { ...review, suggestions: list.map((x) => (x.id === s.id ? r.body : x)) }
  }

  async function applyAccepted() {
    applying = true; err = ''
    const r = await api.aiApply(review.id, autoFix.map((s) => s.id))
    applying = false
    if (!r.ok) { err = r.body.error || 'สร้างฉบับร่างไม่สำเร็จ'; return }
    dispatch('apply', r.body)
  }
</script>

<div class="ai-head">
  <span class="inline-flex items-center gap-1.5 font-bold text-brand"><Icon name="lightbulb" size={16} /> ผู้ช่วย AI</span>
  {#if status?.enabled}<span class="mnote !m-0">{status.model}</span>{/if}
  <span class="flex-1"></span>
  {#if status?.enabled && !readonly}
    <button class="mbtn {review ? 'gh' : 'accent'} inline-flex items-center gap-1.5" disabled={busy} on:click={run}>
      <Icon name="refresh-cw" size={14} /> {busy ? 'AI กำลังอ่านเอกสาร…' : review ? 'ตรวจใหม่' : kind === 'doc' ? 'ให้ AI ตรวจและแนะนำ' : 'ให้ AI สรุปการเปลี่ยนแปลง'}
    </button>
  {/if}
</div>

{#if loading}
  <div class="loadwrap"><span class="spin"></span></div>
{:else if !status?.enabled}
  <div class="empty-state"><Icon name="lightbulb" size={30} stroke={1.5} /><p>ยังไม่ได้เปิดผู้ช่วย AI — ผู้ดูแลระบบต้องตั้งค่า <code>OPENAI_API_KEY</code> บนเซิร์ฟเวอร์</p></div>
{:else}
  <p class="mnote">
    {kind === 'doc' ? 'AI อ่านเนื้อหาเอกสาร ข้อมูลทะเบียน และผลตรวจเลขฉบับแล้วเสนอแนะ' : 'AI เทียบฉบับปัจจุบันกับฉบับร่าง แล้วสรุปสิ่งที่เปลี่ยนและประเด็นที่ควรตรวจ'}
    — เป็นเพียงข้อแนะนำ คุณเลือกเองว่าจะทำตามข้อไหน · เนื้อหาเอกสารถูกส่งไปประมวลผลที่ OpenAI
  </p>
  {#if err}<div class="merr">{err}</div>{/if}
  {#if busy}<div class="loadwrap"><span class="spin"></span><p class="mnote">AI กำลังอ่านเอกสาร อาจใช้เวลา 10–60 วินาที…</p></div>{/if}
  {#if review && !busy}
    {#if review.stale}
      <div class="ver-warn"><Icon name="alert-triangle" size={16} /><span>{kind === 'doc' ? 'เอกสารมีเวอร์ชันใหม่' : 'ฉบับร่างถูกส่งใหม่'}หลังจาก AI ตรวจ — ข้อแนะนำนี้อาจไม่ตรงกับไฟล์ปัจจุบัน</span>{#if !readonly}<button class="vw-btn" on:click={run}>ตรวจใหม่</button>{/if}</div>
    {/if}
    <div class="ai-summary">
      <div class="mnote !m-0 mb-1">สรุปโดย AI · {review.createdName} · {fmtWhen(review.createdAt)}</div>
      <div class="whitespace-pre-line">{review.summary || '—'}</div>
    </div>
    {#if list.length}
      <div class="mnote">ทำตาม {accepted.length} · ไม่ทำตาม {rejected} · ยังไม่ตัดสินใจ {list.length - accepted.length - rejected}</div>
      {#each list as s (s.id)}
        <div class="ai-card" class:acc={s.status === 'accepted'} class:rej={s.status === 'rejected'}>
          <div class="flex flex-wrap items-center gap-1.5 mb-1">
            <span class="ia-res {SEV[s.severity]?.[1] || ''}">{SEV[s.severity]?.[0] || s.severity}</span>
            <span class="ia-res">{CAT[s.category] || s.category}</span>
            {#if s.location}<span class="mnote !m-0">{s.location}</span>{/if}
          </div>
          <div class="font-bold text-[13px]">{s.issue}</div>
          <div class="text-[13px] mt-1">{s.suggestion}</div>
          {#if s.find}
            <div class="ai-diff">
              <div><span class="lbl">เดิม</span><del>{s.find}</del></div>
              <div><span class="lbl">ใหม่</span><ins>{s.replace || '(ลบข้อความนี้)'}</ins></div>
            </div>
          {/if}
          <div class="flex flex-wrap items-center gap-2 mt-2">
            {#if !readonly}
              <button class="st-btn {s.status === 'accepted' ? 'active' : ''}" on:click={() => decide(s, 'accepted')}><Icon name="circle-check" size={13} /> ทำตาม</button>
              <button class="st-btn {s.status === 'rejected' ? 'active' : ''}" on:click={() => decide(s, 'rejected')}><Icon name="x" size={13} /> ไม่ทำตาม</button>
            {/if}
            {#if s.decidedName}<span class="mnote !m-0">{s.status === 'accepted' ? 'ทำตาม' : 'ไม่ทำตาม'} โดย {s.decidedName} · {fmtWhen(s.decidedAt)}</span>{/if}
          </div>
        </div>
      {/each}
      {#if kind === 'doc' && accepted.length}
        <div class="ai-foot">
          {#if canApply && autoFix.length && !review.stale}
            <button class="mbtn accent inline-flex items-center gap-1.5" disabled={applying} on:click={applyAccepted}>
              <Icon name="pencil" size={14} /> {applying ? 'กำลังสร้างฉบับร่าง…' : `สร้างคำขอแก้ไขจากข้อที่ทำตาม (${autoFix.length} ข้อแก้ในไฟล์ได้)`}
            </button>
          {/if}
          <span class="mnote !m-0">
            {#if !canApply}{blockedReason || 'แก้เอกสารนี้ผ่านปุ่ม “แก้ไขเอกสาร” ตามข้อที่ทำตาม'}
            {:else if accepted.length > autoFix.length}{accepted.length - autoFix.length} ข้อที่ทำตามต้องแก้เองในตัวแก้ไขเอกสาร
            {:else}ระบบแก้ข้อความในไฟล์ให้ แล้วส่งเป็นคำขอเปลี่ยนแปลง (DCR) — ต้องลงนาม PIN และผ่านผู้อนุมัติตามปกติ{/if}
          </span>
        </div>
      {/if}
    {:else}
      <div class="empty-state"><Icon name="circle-check" size={30} stroke={1.5} /><p>AI ไม่มีข้อแนะนำเพิ่มเติม</p></div>
    {/if}
  {/if}
{/if}

<style>
  .ai-head { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; flex-wrap: wrap; }
  .ai-summary { background: #EAF1FB; border-radius: 8px; padding: 10px 12px; font-size: 13px; margin-bottom: 10px; }
  .ai-card { border: 1px solid #E6EAF0; border-radius: 8px; padding: 10px 12px; margin-bottom: 8px; background: #fff; }
  .ai-card.acc { border-color: #1F9D63; background: #F4FBF7; }
  .ai-card.rej { opacity: .6; }
  .ai-diff { margin-top: 6px; font-size: 12.5px; background: #F5F7FA; border-radius: 6px; padding: 6px 8px; display: grid; gap: 3px; }
  .ai-diff .lbl { display: inline-block; min-width: 34px; color: #667085; font-size: 11px; }
  .ai-diff del { color: #C0504D; overflow-wrap: anywhere; }
  .ai-diff ins { color: #1F9D63; text-decoration: none; overflow-wrap: anywhere; }
  .ai-foot { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; margin-top: 10px; padding-top: 10px; border-top: 1px solid #E6EAF0; }
</style>
