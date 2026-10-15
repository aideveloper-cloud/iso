<script>
  // คำขอเปลี่ยนแปลงเอกสาร (DCR) หนึ่งใบ: ฉบับร่าง + ขั้นตอนลงนาม + ปุ่มตรวจสอบ/อนุมัติ/ตีกลับ/ส่งใหม่/ยกเลิก
  import { createEventDispatcher } from 'svelte'
  import { api, changeFileUrl, bytesToB64 } from './lib/api.js'
  import { DCR_STATUS, dcrStepLabel, noteParts, fmtWhen } from './lib/changeNote.js'
  import DocBody from './DocBody.svelte'
  import SignDialog from './SignDialog.svelte'
  import DecisionDialog from './DecisionDialog.svelte'
  import EditActions from './EditActions.svelte'
  import OfficeEdit from './OfficeEdit.svelte'
  import Icon from './Icon.svelte'

  export let change // รายละเอียดจาก /api/changes/:id (มี steps, can)
  export let doc
  export let me = null
  const dispatch = createEventDispatcher()

  let content = null, contentErr = '', loadingContent = false
  let editing = false, dirty = false
  let body // DocBody
  let deciding = null // { kind: 'pass'|'return'|'cancel' }
  let resubmit = null // { dataBase64?, ext?, filename?, source?, editId?, label }
  let gEdit = false // แก้ฉบับร่างที่ถูกตีกลับในตัวแก้ไขบนเว็บ (ONLYOFFICE)
  let uploadInput
  let err = ''

  $: np = noteParts(change)
  $: can = change?.can || {}
  $: lastReturn = (change?.steps || []).filter((s) => s.action === 'return').slice(-1)[0]
  $: canWebOffice = can.resubmit && ['docx', 'xlsx', 'xls'].includes(change?.ext)
  $: editableDraft = can.resubmit && content && (content.kind === 'html' || content.kind === 'sheet')
  $: if (change?.id) loadContent(change.id, change.sha256)

  let loadedKey = ''
  async function loadContent(id, sha) {
    const key = `${id}:${sha}`
    if (key === loadedKey) return
    loadedKey = key
    loadingContent = true
    contentErr = ''
    content = null
    editing = false
    dirty = false
    try {
      content = await api.changeContent(id)
      if (content?.error) throw new Error(content.error)
    } catch (e) {
      content = null
      contentErr = 'เปิดฉบับร่างไม่สำเร็จ: ' + e.message
    }
    loadingContent = false
  }

  const DECIDE = {
    review_pass: { title: 'ผู้ตรวจสอบ: ผ่าน', label: 'ผ่าน — ส่งต่อผู้อนุมัติ', comment: 'ความเห็นผู้ตรวจสอบ' },
    approve_pass: { title: 'ผู้อนุมัติ: อนุมัติให้มีผลบังคับใช้', label: 'อนุมัติ — มีผลบังคับใช้', comment: 'ความเห็นผู้อนุมัติ' },
    return: { title: 'ตีกลับให้ผู้จัดทำแก้ไข', label: 'ตีกลับ', comment: 'เหตุผลที่ตีกลับ', required: true, danger: true },
    cancel: { title: 'ยกเลิกคำขอเปลี่ยนแปลง', label: 'ยกเลิกคำขอ', comment: 'เหตุผลการยกเลิกคำขอ', required: true, danger: true, noPin: true },
  }
  $: dd = deciding ? DECIDE[deciding] : null

  async function onDecide(e) {
    const { comment, pin, fail } = e.detail
    let r
    if (deciding === 'cancel') r = await api.changeCancel(change.id, comment)
    else r = await api.changeAct(change.id, deciding === 'return' ? 'return' : 'pass', comment, pin)
    if (!r.ok) { fail(r.body.error || 'บันทึกไม่สำเร็จ'); return }
    const wasApprove = deciding === 'approve_pass'
    deciding = null
    change = r.body
    dispatch('changed', { change: r.body, effective: wasApprove && r.body.status === 'effective' })
  }

  async function startResubmitFromEdit() {
    err = ''
    try {
      const d = await body.buildEdited()
      resubmit = {
        dataBase64: bytesToB64(d.bytes), ext: d.ext, source: 'edit',
        filename: (change.filename || doc.file).replace(/\.[^.]+$/, '') + '.' + d.ext,
        label: 'แก้ไขฉบับร่างบนเว็บ แล้วส่งใหม่',
      }
    } catch (e) {
      err = 'สร้างไฟล์ที่แก้ไขไม่สำเร็จ: ' + e.message
    }
  }

  function onPickFile(e) {
    const f = e.target.files?.[0]
    e.target.value = ''
    if (!f) return
    if (f.size > 40 * 1024 * 1024) { err = 'ไฟล์ใหญ่เกิน 40 MB'; return }
    const rd = new FileReader()
    rd.onload = () => {
      resubmit = {
        dataBase64: String(rd.result).split(',')[1],
        ext: (f.name.split('.').pop() || '').toLowerCase(),
        filename: f.name, source: 'upload',
        label: `แนบไฟล์ “${f.name}” แทนฉบับร่างเดิม แล้วส่งใหม่`,
      }
    }
    rd.readAsDataURL(f)
  }

  async function onResubmit(e) {
    const { note, reason, newRev, pin, fail } = e.detail
    const { label, ...file } = resubmit
    const r = await api.changeResubmit(change.id, { ...file, note, reason, newRev, pin })
    if (!r.ok) { fail(r.body.error || 'ส่งใหม่ไม่สำเร็จ'); return }
    resubmit = null
    gEdit = false
    change = r.body
    dispatch('changed', { change: r.body })
  }

  const shortSha = (s) => (s ? s.slice(0, 12) : '—')
</script>

<div class="dcr">
  <div class="dcr-head">
    <div class="flex flex-wrap items-center gap-2">
      <b class="text-[15px]">{change.refNo}</b>
      <span class="status-badge dcr-{change.status}">{DCR_STATUS[change.status] || change.status}</span>
      <span class="mut">{change.fromRev || '—'} → <b>{change.newRev}</b></span>
      <span class="flex-1"></span>
      <a class="mbtn gh inline-flex items-center gap-1.5" href={changeFileUrl(change.id, true)} download><Icon name="download" size={14} /> ดาวน์โหลดฉบับร่าง</a>
    </div>
    <div class="det-info mt-2">
      <div><b>เหตุผลการเปลี่ยนแปลง:</b> {change.reason || '—'}</div>
      {#if np.topic}<div><b>เรื่องที่แก้ไข:</b> {np.topic}</div>{/if}
      {#if np.detail}<div><b>รายละเอียด:</b> {np.detail}</div>{/if}
      {#if np.pages}<div><b>หน้าที่แก้ไข:</b> {np.pages}</div>{/if}
      <div><b>ผู้จัดทำ:</b> {change.preparedName}{#if change.preparedPosition} · {change.preparedPosition}{/if} · {fmtWhen(change.createdAt)}</div>
      <div><b>ไฟล์:</b> {change.filename} <span class="mut">· SHA-256 {shortSha(change.sha256)}</span></div>
      {#if change.status === 'effective'}<div><b>มีผลเป็นเวอร์ชัน:</b> v{change.version}</div>{/if}
    </div>
  </div>

  {#if change.status === 'returned' && lastReturn}
    <div class="merr">ถูกตีกลับโดย {lastReturn.actorName}: {lastReturn.comment}</div>
  {/if}
  {#if err}<div class="merr">{err}</div>{/if}

  <div class="dcr-actions">
    {#if can.review}
      <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => (deciding = 'review_pass')}><Icon name="circle-check" size={14} /> ตรวจสอบผ่าน</button>
      <button class="mbtn danger" on:click={() => (deciding = 'return')}>ตีกลับ</button>
    {/if}
    {#if can.approve}
      <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => (deciding = 'approve_pass')}><Icon name="circle-check" size={14} /> อนุมัติให้มีผลบังคับใช้</button>
      <button class="mbtn danger" on:click={() => (deciding = 'return')}>ตีกลับ</button>
    {/if}
    {#if can.resubmit}
      {#if canWebOffice && !editing}
        <button class="mbtn {gEdit ? 'gh' : 'accent'} inline-flex items-center gap-1.5" on:click={() => (gEdit = !gEdit)}>
          <Icon name={gEdit ? 'eye' : 'pencil'} size={14} /> {gEdit ? 'ดูฉบับร่างที่ส่งไว้' : 'แก้ไขฉบับร่าง'}
        </button>
      {/if}
      {#if editableDraft && !gEdit}
        <EditActions {editing} {dirty} updateLabel="ส่งใหม่เพื่อตรวจสอบ" editLabel="แก้ไขฉบับร่างบนเว็บ" on:cancel={() => { editing = false; dirty = false; loadedKey = ''; loadContent(change.id, change.sha256) }} on:start={() => { editing = true; dirty = false }} on:update={startResubmitFromEdit} />
      {/if}
      <button class="mbtn inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="cloud-upload" size={14} /> แนบไฟล์ใหม่</button>
      <input bind:this={uploadInput} type="file" class="hidden" on:change={onPickFile} />
      {#if !editing}
        <button class="mbtn gh" on:click={() => (resubmit = { label: 'ส่งฉบับร่างเดิมใหม่อีกครั้ง' })}>ส่งใหม่โดยไม่เปลี่ยนไฟล์</button>
      {/if}
    {/if}
    {#if can.cancel && !editing}
      <button class="mbtn gh" on:click={() => (deciding = 'cancel')}>ยกเลิกคำขอ</button>
    {/if}
    {#if !can.review && !can.approve && (change.status === 'review' || change.status === 'approve')}
      <span class="mnote">
        {change.preparedBy === me?.empCode ? 'รอผู้อื่นตรวจสอบ/อนุมัติ — ผู้จัดทำลงนามขั้นนี้เองไม่ได้' : `รอ${change.status === 'review' ? 'ผู้ตรวจสอบ' : 'ผู้อนุมัติ'}ลงนาม`}
      </span>
    {/if}
  </div>

  <h4 class="dcr-sub">ขั้นตอนการลงนาม</h4>
  <table class="chg-table">
    <thead><tr><th>ขั้นตอน</th><th>ผู้ลงนาม</th><th>วันเวลา</th><th>ความเห็น</th><th>SHA-256 ไฟล์ที่ลงนาม</th></tr></thead>
    <tbody>
      {#each change.steps || [] as s}
        <tr>
          <td>{dcrStepLabel(s)}</td>
          <td>{s.actorName}{#if s.actorPosition}<br /><span class="mut">{s.actorPosition}</span>{/if}</td>
          <td class="nowrap">{fmtWhen(s.at)}</td>
          <td>{s.comment || '—'}</td>
          <td class="mono">{shortSha(s.sha256)}</td>
        </tr>
      {/each}
    </tbody>
  </table>

  <h4 class="dcr-sub">ฉบับร่าง {change.newRev}{#if change.status !== 'effective'} <span class="mut">— ยังไม่มีผลบังคับใช้</span>{/if}</h4>
  {#if gEdit && canWebOffice}
    <OfficeEdit code={doc.code} changeId={change.id} submitLabel="ส่งใหม่เพื่อตรวจสอบ"
      on:submit={(e) => (resubmit = { editId: e.detail.editId, label: 'ฉบับที่แก้ในตัวแก้ไขเอกสาร → ส่งใหม่เพื่อตรวจสอบ' })}
      on:closed={() => (gEdit = false)} />
  {:else if loadingContent}
    <div class="loadwrap"><span class="spin"></span></div>
  {:else if contentErr}
    <div class="merr">{contentErr}</div>
  {:else if content}
    <div class="dcr-draft" class:is-draft={change.status !== 'effective'}>
      <DocBody
        bind:this={body}
        {content}
        name={doc.name}
        origUrl={changeFileUrl(change.id)}
        embedUrl={changeFileUrl(change.id)}
        downloadUrl={changeFileUrl(change.id, true)}
        {editing}
        saveLabel="ส่งใหม่เพื่อตรวจสอบ"
        bind:dirty
      />
    </div>
  {/if}
</div>

{#if dd}
  <DecisionDialog
    title={dd.title}
    message="{change.refNo} · {doc.code} {doc.name} · {change.fromRev || '—'} → {change.newRev}"
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

{#if resubmit}
  <SignDialog
    title="{doc.code} · {doc.name}"
    summary={resubmit.label}
    confirmLabel="ส่งใหม่เพื่อตรวจสอบ"
    {me}
    withPin
    change={{ fromRev: change.fromRev, newRev: change.newRev }}
    initialNote={change.note}
    initialReason={change.reason}
    on:confirm={onResubmit}
    on:cancel={() => (resubmit = null)}
  />
{/if}
