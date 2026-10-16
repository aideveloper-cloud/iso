<script>
  import { createEventDispatcher, onMount, onDestroy, tick } from 'svelte'
  import { srcLabel, fmtWhen, DCR_STATUS } from './lib/changeNote.js'
  import { api, fileUrl, pdfUrl, bytesToB64 } from './lib/api.js'
  import { OFFICE_EXTS } from './lib/const.js'
  import DocBody from './DocBody.svelte'
  import SignDialog from './SignDialog.svelte'
  import ConfirmDialog from './ConfirmDialog.svelte'
  import ChangeHistory from './ChangeHistory.svelte'
  import EditActions from './EditActions.svelte'
  import ChangePanel from './ChangePanel.svelte'
  import OfficeEdit from './OfficeEdit.svelte'
  import DocAcks from './DocAcks.svelte'
  import DecisionDialog from './DecisionDialog.svelte'
  import Icon from './Icon.svelte'

  export let doc // { code, name, deptName, rev, file }
  export let me = null
  /** เปิดมาที่คำขอเปลี่ยนแปลง (DCR) ใบนี้ */
  export let initialChange = null
  const dispatch = createEventDispatcher()

  let changes = [] // DCR ของเอกสารนี้ (ใหม่ → เก่า)
  let selChange = null // id ของ DCR ที่เปิดดู
  let statusAct = null // 'obsolete' | 'reinstate'
  let ackData = null // การแจกจ่าย/รับทราบ (/api/docs/:code/acks)
  let acking = false
  async function loadAcks() { try { ackData = await api.docAcks(doc.code) } catch (e) { ackData = null } }
  $: myAck = ackData?.mine
  async function onAck(e) {
    const { comment, pin, fail } = e.detail
    const r = await api.ackSign(myAck.id, pin, comment)
    if (!r.ok) { fail(r.body.error || 'บันทึกไม่สำเร็จ'); return }
    acking = false
    savedMsg = `รับทราบ ${myAck.rev} แล้ว — บันทึกในประวัติการอบรมของคุณ`
    await loadAcks()
    dispatch('saved')
  }

  let meta = null, ver = null, content = null
  let dirty = false, editing = false, tab = 'doc'
  let manage = false // false = โหมดอ่าน (ค่าเริ่มต้น) · true = แสดงเครื่องมือขอแก้ไข/แนบไฟล์/ยกเลิกใช้
  let signing = null // 'edit' | 'upload' | 'office' | null
  let gEdit = false // แสดงตัวแก้ไขเอกสารบนเว็บ (ONLYOFFICE) แทนตัวเอกสาร
  let gEditId = null // รอบแก้ไขที่กำลังส่งขออนุมัติ
  let pendingUpload = null
  let err = '', loading = true
  let uploadInput
  let confirmAction = null // { message, run }
  let body // DocBody
  let draft = null // { bytes, ext } ไฟล์ที่แก้แล้ว สร้างตอนกดอัปเดท
  let savedMsg = ''

  $: latestVer = meta?.versions[0]?.version
  $: isLatest = meta != null && ver === latestVer
  $: isSheetExt = meta?.ext === 'xls' || meta?.ext === 'xlsx'
  $: chronVersions = meta ? [...meta.versions].reverse() : []
  $: openChange = changes.find((c) => ['review', 'approve', 'returned'].includes(c.status)) || null
  $: obsolete = meta?.docStatus === 'obsolete'
  // แก้เอกสารควบคุมได้เฉพาะผ่าน DCR: ไม่มีคำขอค้าง, ไม่ถูกยกเลิกใช้, ไม่ใช่บัญชีอ่านอย่างเดียว
  $: canChange = !!meta && isLatest && !openChange && !obsolete && me?.role !== 'viewer'
  $: canWebEdit = canChange && meta?.editable && content && content.kind !== 'download' && content.kind !== 'embed'
  // ตัวแก้ไขบนเว็บ (ONLYOFFICE) แก้ได้เฉพาะ Word/Excel
  $: canWebOffice = canChange && OFFICE_EXTS.includes(meta?.ext)
  $: canObsolete = !obsolete && !openChange && (me?.role === 'approver' || me?.role === 'admin')
  $: canReinstate = obsolete && me?.role === 'admin'
  $: canManage = canChange || canObsolete
  $: shownChange =changes.find((c) => c.id === selChange) || openChange || changes[0] || null

  function askConfirm(message, run) {
    confirmAction = { message, run }
  }

  function cancelEdit() {
    editing = false
    dirty = false
    load(ver)
  }

  function startEdit() {
    manage = true
    editing = true
    dirty = false
  }

  function isLatestVer(v) {
    return v.version === latestVer
  }

  onMount(() => {
    load().then(loadAcks).then(resumeWebOffice)
    loadChanges().then(() => {
      if (initialChange && changes.some((c) => c.id === initialChange)) { selChange = initialChange; tab = 'dcr' }
    })
    window.addEventListener('keydown', onKey)
  })
  onDestroy(() => window.removeEventListener('keydown', onKey))

  function onKey(e) {
    if (e.key === 'Escape' && !signing && !confirmAction && !statusAct && !gEdit) close()
  }

  /** มีรอบแก้ไขค้างอยู่ (เช่น รีเฟรชหน้าระหว่างแก้) → เปิดตัวแก้ไขต่อให้เลย งานที่บันทึกไว้ไม่หาย */
  async function resumeWebOffice() {
    try {
      const s = await api.editMine(doc.code)
      if (s && !s.changeId) openWebOffice()
    } catch (e) { /* ไม่มีรอบค้าง */ }
  }

  function openWebOffice() {
    tab = 'doc'
    manage = true
    gEdit = true
  }

  async function loadChanges() {
    try { changes = await api.docChanges(doc.code) } catch (e) { changes = [] }
  }

  async function onChangeUpdated(e) {
    const c = e.detail.change
    await loadChanges()
    if (e.detail.effective) {
      savedMsg = `อนุมัติแล้ว — ${c.newRev} มีผลบังคับใช้เป็นเวอร์ชัน ${c.version}`
      await load(c.version)
    } else {
      await load(ver)
    }
    selChange = c.id
    tab = 'dcr'
    dispatch('saved')
  }

  async function onStatusAct(e) {
    const { comment, pin, fail } = e.detail
    const r = statusAct === 'obsolete'
      ? await api.docObsolete(doc.code, comment, pin)
      : await api.docReinstate(doc.code, comment, pin)
    if (!r.ok) { fail(r.body.error || 'บันทึกไม่สำเร็จ'); return }
    savedMsg = statusAct === 'obsolete' ? 'ประกาศยกเลิกใช้เอกสารแล้ว' : 'นำเอกสารกลับมาใช้แล้ว'
    statusAct = null
    await load(ver)
    dispatch('saved')
  }

  async function load(v) {
    loading = true
    err = ''
    content = null
    dirty = false
    editing = false
    try {
      meta = await api.versions(doc.code)
      ver = v || meta.versions[0]?.version
      try {
        content = await api.content(doc.code, ver)
        if (content?.error) throw new Error(content.error)
      } catch (e) {
        content = null
        err = (meta?.missing || doc.missing)
          ? 'ไฟล์ต้นฉบับหายจากดิสก์ — อัปโหลดเวอร์ชันใหม่เข้าสู่ระบบเพื่อใช้งานต่อ'
          : 'เปิดเอกสารไม่สำเร็จ: ' + e.message
      }
    } catch (e) {
      err = 'เปิดเอกสารไม่สำเร็จ: ' + e.message
    }
    loading = false
  }

  async function beginUpdate() {
    err = ''
    try {
      draft = await body.buildEdited()
      signing = 'edit'
    } catch (e) {
      err = 'สร้างไฟล์ที่แก้ไขไม่สำเร็จ: ' + e.message
    }
  }

  function onPickFile(e) {
    const f = e.target.files?.[0]
    e.target.value = ''
    if (!f) return
    if (f.size > 40 * 1024 * 1024) {
      err = 'ไฟล์ใหญ่เกิน 40 MB'
      return
    }
    const rd = new FileReader()
    rd.onload = () => {
      pendingUpload = {
        name: f.name,
        ext: (f.name.split('.').pop() || '').toLowerCase(),
        b64: String(rd.result).split(',')[1],
        size: f.size,
      }
      signing = 'upload'
    }
    rd.readAsDataURL(f)
  }

  async function doSave(e) {
    const { note, reason, newRev, pin, fail } = e.detail
    try {
      let dataBase64, ext, source, filename
      if (signing === 'office') {
        const { ok, body } = await api.changeCreate(doc.code, { editId: gEditId, note, reason, newRev, pin })
        if (!ok) { fail(body.error || 'ส่งคำขอไม่สำเร็จ'); return }
        signing = null
        gEdit = false
        gEditId = null
        await afterSubmit(body)
        return
      }
      if (signing === 'edit') {
        dataBase64 = bytesToB64(draft.bytes)
        ext = draft.ext
        source = 'edit'
        filename = doc.file.replace(/\.[^.]+$/, '') + '.' + ext
      } else {
        dataBase64 = pendingUpload.b64
        ext = pendingUpload.ext
        source = 'upload'
        filename = pendingUpload.name
      }
      const { ok, body } = await api.changeCreate(doc.code, { dataBase64, ext, source, filename, note, reason, newRev, pin })
      if (!ok) {
        fail(body.error || 'ส่งคำขอไม่สำเร็จ')
        return
      }
      signing = null
      pendingUpload = null
      draft = null
      await afterSubmit(body)
    } catch (ex) {
      fail('ส่งคำขอไม่สำเร็จ: ' + ex.message)
    }
  }

  async function afterSubmit(body) {
    savedMsg = `ส่งคำขอ ${body.refNo} แล้ว — ${body.newRev} จะมีผลบังคับใช้เมื่อผู้ตรวจสอบและผู้อนุมัติลงนามครบ`
    await load(ver)
    await loadChanges()
    selChange = body.id
    tab = 'dcr'
    dispatch('saved')
    setTimeout(() => { if (savedMsg.includes(body.refNo)) savedMsg = '' }, 8000)
  }

  function close() {
    if (!dirty) {
      dispatch('close')
      return
    }
    askConfirm('มีการแก้ไขที่ยังไม่ได้บันทึก ปิดหน้าต่างเลยหรือไม่?', () => dispatch('close'))
  }

  function cancelSign() {
    signing = null
    pendingUpload = null
  }

  async function openAndEdit(v) {
    tab = 'doc'
    await load(v)
    if (canWebOffice) openWebOffice()
    else if (meta?.editable) await startEdit()
  }

  function openVersion(v) {
    tab = 'doc'
    load(v)
  }

</script>

<div class="mbg" on:mousedown|self={close}>
  <div class="mbox wide">
    <header class="mhead">
      <div class="mt">
        <b>{doc.code}</b> {doc.name}
        {#if meta && obsolete}<span class="status-obsolete">Obsolete</span>{:else if meta && isLatest}<span class="status-active">Active</span>{/if}
        <div class="msub">
          {doc.deptName} · {meta?.rev || doc.rev}{#if meta?.eff} · บังคับใช้ {meta.eff}{/if}
          {#if meta}
            {#if isLatest}
              <span class="ver-badge latest inline-flex items-center gap-1"><Icon name="circle-check" size={12} /> เวอร์ชันล่าสุด v{ver}</span>
            {:else}
              <span class="ver-badge old inline-flex items-center gap-1"><Icon name="alert-triangle" size={12} /> เวอร์ชันเก่า v{ver} · ล่าสุด v{latestVer}</span>
            {/if}
          {/if}
        </div>
      </div>
      <button class="mx inline-flex items-center justify-center" on:click={close}><Icon name="x" size={16} /></button>
    </header>

    <nav class="mtabs">
      <button class={tab === 'doc' ? 'active' : ''} on:click={() => (tab = 'doc')}>เอกสาร</button>
      <button class={tab === 'hist' ? 'active' : ''} on:click={() => (tab = 'hist')}>ประวัติเวอร์ชัน ({meta?.versions.length || 0})</button>
      <button class={tab === 'dcr' ? 'active' : ''} on:click={() => (tab = 'dcr')}>คำขอเปลี่ยนแปลง ({changes.length}){#if openChange} •{/if}</button>
      <button class={tab === 'ack' ? 'active' : ''} on:click={() => { tab = 'ack'; loadAcks() }}>การแจกจ่าย/รับทราบ{#if myAck} •{/if}</button>
      <span class="grow"></span>
      {#if !loading && tab === 'doc' && meta}
        {#if content}<a class="mbtn gh inline-flex items-center gap-1.5" href={fileUrl(doc.code, ver, true)} download><Icon name="download" size={14} /> ดาวน์โหลดเอกสาร</a>{/if}
        {#if editing}
          <EditActions {editing} {dirty} updateLabel="ส่งขออนุมัติ" on:cancel={cancelEdit} on:start={startEdit} on:update={beginUpdate} />
        {:else if manage}
          <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => { manage = false; gEdit = false }}><Icon name="eye" size={14} /> กลับโหมดอ่าน</button>
        {:else if canManage}
          {#if canWebOffice}
            <button class="mbtn accent inline-flex items-center gap-1.5" on:click={openWebOffice}><Icon name="pencil" size={14} /> แก้ไขเอกสาร</button>
          {/if}
          <button class="mbtn {canWebOffice ? 'gh' : 'accent'} inline-flex items-center gap-1.5" on:click={() => (manage = true)}><Icon name="pencil" size={14} /> {canWebOffice ? 'ตัวเลือกอื่น' : 'ขอแก้ไขเอกสาร'}</button>
        {/if}
      {/if}
    </nav>
    <input bind:this={uploadInput} type="file" class="hidden" on:change={onPickFile} />

    <div class="mbody">
      {#if savedMsg}<div class="save-ok inline-flex items-center gap-1.5"><Icon name="circle-check" size={16} /> {savedMsg}</div>{/if}
      {#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={() => load(ver)}>ลองใหม่</button></div>{/if}
      {#if myAck && !obsolete}
        <div class="ack-banner">
          <Icon name="clipboard-check" size={16} />
          <span>คุณต้อง<b>รับทราบเอกสารนี้ {myAck.rev}</b>{#if myAck.dueDate} ภายใน {myAck.dueDate}{/if} — อ่านเนื้อหาด้านล่างให้ครบแล้วกด “อ่านและเข้าใจแล้ว”{#if myAck.overdue} <b class="text-danger">(เลยกำหนด)</b>{/if}</span>
          <button class="vw-btn" disabled={myAck.version !== ver} title={myAck.version !== ver ? 'เปิดฉบับที่ต้องรับทราบก่อน' : ''} on:click={() => (acking = true)}>อ่านและเข้าใจแล้ว</button>
        </div>
      {/if}
      {#if meta && obsolete}
        <div class="obs-warn">
          <Icon name="alert-triangle" size={16} />
          <span><b>เอกสารนี้ยกเลิกใช้แล้ว</b> — ห้ามนำไปใช้งาน เก็บไว้เพื่ออ้างอิงเท่านั้น
            {#if meta.obsolete}<br /><span class="mut">โดย {meta.obsolete.by} · {fmtWhen(meta.obsolete.at)} · เหตุผล: {meta.obsolete.reason}</span>{/if}</span>
          {#if canReinstate}<button class="vw-btn" on:click={() => (statusAct = 'reinstate')}>นำกลับมาใช้</button>{/if}
        </div>
      {/if}
      {#if openChange && tab !== 'dcr'}
        <div class="ver-warn">
          <Icon name="clock" size={16} />
          <span>มีคำขอ <b>{openChange.refNo}</b> ({openChange.fromRev || '—'} → {openChange.newRev}) สถานะ <b>{DCR_STATUS[openChange.status]}</b> โดย {openChange.preparedName} — ฉบับที่แสดงอยู่นี้ยังเป็นฉบับใช้งานปัจจุบัน</span>
          <button class="vw-btn" on:click={() => { selChange = openChange.id; tab = 'dcr' }}>เปิดคำขอ</button>
        </div>
      {/if}
      {#if loading}
        <div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังเปิดเอกสาร…</p></div>
      {:else if tab === 'doc' && meta}
        {#if !manage && !editing}
          <div class="read-bar">
            <span class="read-mode inline-flex items-center gap-1.5"><Icon name="eye" size={14} /> โหมดอ่าน</span>
            <span class="mut">อ่านอย่างเดียว — ต้องการแก้ไขให้กด “{canWebOffice ? 'แก้ไขเอกสาร' : 'ขอแก้ไขเอกสาร'}” (ผ่านคำขอ DCR และการอนุมัติ)</span>
            <span class="grow"></span>
            {#if doc.type === 'FM'}
              <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => dispatch('eform', { code: doc.code, mode: 'new' })}><Icon name="plus" size={14} /> กรอกแบบฟอร์มใหม่</button>
              <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => dispatch('eform', { code: doc.code, mode: 'list' })}>บันทึกที่กรอกแล้ว</button>
            {/if}
            <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => (tab = 'hist')}><Icon name="clock" size={14} /> ประวัติ ({meta.versions.length})</button>
          </div>
        {:else if !gEdit}
        <div class="file-tools">
          <div class="ft-head">
            <b>การแก้ไขเอกสาร</b>
            <span class="mut">{canWebOffice ? 'แก้ไขเอกสาร' : 'แก้บนเว็บหรือแนบไฟล์'} → ส่งคำขอ (DCR) → ตรวจสอบ → อนุมัติ → มีผลบังคับใช้เป็น Rev. ใหม่</span>
          </div>
          <div class="ft-actions">
            {#if canWebOffice && !gEdit}
              <button class="mbtn accent inline-flex items-center gap-1.5" on:click={openWebOffice}><Icon name="pencil" size={14} /> แก้ไขเอกสาร</button>
            {/if}
            {#if gEdit}
              <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => (gEdit = false)}><Icon name="eye" size={14} /> ดูฉบับใช้งานปัจจุบัน / ตัวเลือกอื่น</button>
            {:else if canWebEdit && !canWebOffice}
              <EditActions {editing} {dirty} accent updateLabel="ส่งขออนุมัติ" editLabel="แก้ไขบนเว็บเพื่อขออนุมัติ" on:cancel={cancelEdit} on:start={startEdit} on:update={beginUpdate} />
            {:else if canChange}
              <span class="ft-hint">ไฟล์ชนิดนี้แก้บนเว็บไม่ได้ — ใช้แนบไฟล์ใหม่แทน</span>
            {:else if openChange}
              <span class="ft-hint">มีคำขอ {openChange.refNo} ค้างอยู่ — แก้ไขต่อได้ในแท็บ “คำขอเปลี่ยนแปลง”</span>
            {:else if obsolete}
              <span class="ft-hint">เอกสารยกเลิกใช้ — แก้ไขไม่ได้</span>
            {/if}
            {#if !gEdit}
            {#if canChange}
              <button class="mbtn inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="cloud-upload" size={14} /> แนบไฟล์ขอแก้ไข</button>
            {/if}
            {#if canObsolete}
              <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => (statusAct = 'obsolete')}><Icon name="x" size={14} /> ยกเลิกใช้เอกสาร</button>
            {/if}
            {#if content}<a class="mbtn gh inline-flex items-center gap-1.5" href={fileUrl(doc.code, ver, true)} download><Icon name="download" size={14} /> ดาวน์โหลดเอกสาร</a>{/if}
            {#if doc.type === 'FM'}
              <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => dispatch('eform', { code: doc.code, mode: 'new' })}><Icon name="plus" size={14} /> กรอกแบบฟอร์มใหม่</button>
              <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => dispatch('eform', { code: doc.code, mode: 'list' })}>บันทึกที่กรอกแล้ว</button>
            {/if}
            <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => (tab = 'hist')}><Icon name="clock" size={14} /> ประวัติการแก้ไข ({meta.versions.length})</button>
            {/if}
          </div>
        </div>
        {#if !editing && !gEdit}<ChangeHistory versions={meta.versions || []} currentVer={ver} controlled onDoc />{/if}
        {/if}

        {#if gEdit && canWebOffice}
          <OfficeEdit code={doc.code}
            on:submit={(e) => { gEditId = e.detail.editId; signing = 'office' }}
            on:closed={() => (gEdit = false)} />
        {:else if !content}
          <div class="nopreview">
            <div class="big text-faint flex justify-center"><Icon name="alert-triangle" size={44} stroke={1.5} /></div>
            <p>{err || 'ไม่พบไฟล์ในระบบ'}</p>
            {#if canChange}
              <button class="mbtn inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="upload" size={14} /> แนบไฟล์ใหม่เพื่อขออนุมัติ</button>
            {/if}
          </div>
        {:else}
          {#if !isLatest}
            <div class="ver-warn">
              <Icon name="alert-triangle" size={16} />
              <span>กำลังดูเวอร์ชันเก่า (v{ver}) ซึ่งไม่ควรนำไปใช้งานจริง — เวอร์ชันล่าสุดคือ <b>v{latestVer}</b></span>
              <button class="vw-btn" on:click={() => load(latestVer)}>ไปเวอร์ชันล่าสุด</button>
            </div>
          {/if}
          <DocBody
            bind:this={body}
            {content}
            name={doc.name}
            origUrl={fileUrl(doc.code, ver)}
            embedUrl={fileUrl(doc.code, ver)}
            downloadUrl={fileUrl(doc.code, ver, true)}
            pdfUrl={pdfUrl(doc.code, ver)}
            {editing}
            saveLabel="ส่งขออนุมัติ"
            bind:dirty
          />
        {/if}
      {:else if tab === 'hist' && meta}
        <div class="ver-fan">
          <div class="vf-root">
            <span class="vf-code">{doc.code}</span>
            <span class="vf-name">{doc.name}</span>
          </div>
          <div class="vf-trunk" aria-hidden="true"></div>
          <div class="vf-branches">
            {#each chronVersions as v}
              <button
                type="button"
                class="vf-leaf {v.version === ver ? 'cur' : ''} {isLatestVer(v) ? 'latest' : ''}"
                on:click={() => openVersion(v.version)}
              >
                <span class="vf-ver">Ver {v.version}</span>
                {#if isLatestVer(v)}<span class="vf-tag">แก้ล่าสุด</span>{:else}<span class="vf-tag muted">ข้อมูลเก่า</span>{/if}
              </button>
            {/each}
          </div>
        </div>

        <ChangeHistory versions={meta.versions} currentVer={ver} controlled showSigner>
          <svelte:fragment slot="actions" let:v>
            {#if v.version !== ver}
              <button class="mbtn gh" on:click={() => openVersion(v.version)}>เปิดดู</button>
            {/if}
            {#if isLatestVer(v) && canChange}
              {#if meta.editable || canWebOffice}
                <button class="mbtn accent" on:click={() => openAndEdit(v.version)}>แก้ไขเพื่อขออนุมัติ</button>
              {/if}
              <button class="mbtn" on:click={() => uploadInput.click()}>แนบไฟล์ขอแก้ไข</button>
            {/if}
            <a class="mbtn gh" href={fileUrl(doc.code, v.version, true)} download>ดาวน์โหลด</a>
          </svelte:fragment>
        </ChangeHistory>

        <div class="vtree">
          {#each meta.versions as v}
            <div class="vrow {v.version === ver ? 'cur' : ''}">
              <div class="vcard">
                <div class="vtop">
                  <span class="vn">Ver {v.version}{#if v.rev} · {v.rev}{/if}</span>
                  <span class="vsrc {v.source}">{srcLabel(v.source)}</span>
                  {#if isLatestVer(v)}<span class="ver-badge latest inline-flex items-center gap-1"><Icon name="circle-check" size={12} /> แก้ล่าสุด</span>{/if}
                  {#if v.version === ver}<span class="mnote">· กำลังดู</span>{/if}
                </div>
                <div class="vwho inline-flex items-center gap-1.5"><Icon name="pen-line" size={14} /> <b>{v.signerName}</b>{#if v.signerRole} · {v.signerRole}{/if}
                  {#if v.approvedBy}<span class="mut">· อนุมัติโดย {v.approvedBy}</span>{/if}</div>
                <div class="vmeta inline-flex items-center gap-1.5"><Icon name="clock" size={12} /> {fmtWhen(v.signedAt)} · {v.filename}</div>
                {#if v.note}<div class="vnote inline-flex items-center gap-1.5"><Icon name="note" size={13} /> {v.note}</div>{/if}
                <div class="vbtns">
                  {#if v.version !== ver}
                    <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => openVersion(v.version)}><Icon name="eye" size={14} /> เปิดดู</button>
                  {/if}
                  {#if isLatestVer(v)}
                    {#if canChange}
                      {#if meta.editable || canWebOffice}
                        <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => openAndEdit(v.version)}>
                          <Icon name="pencil" size={14} /> {isSheetExt ? 'แก้ตารางเพื่อขออนุมัติ' : 'แก้ไขเพื่อขออนุมัติ'}
                        </button>
                      {/if}
                      <button class="mbtn inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="cloud-upload" size={14} /> แนบไฟล์ขอแก้ไข</button>
                    {/if}
                  {:else}
                    <span class="mnote">ข้อมูลเก่า — ใช้เพื่ออ้างอิงเท่านั้น</span>
                  {/if}
                  <a class="mbtn gh inline-flex items-center gap-1.5" href={fileUrl(doc.code, v.version, true)} download><Icon name="download" size={14} /> ดาวน์โหลด</a>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {:else if tab === 'ack' && meta}
        {#if ackData}<DocAcks data={ackData} code={doc.code} on:changed={(e) => (ackData = e.detail)} />{:else}<div class="loadwrap"><span class="spin"></span></div>{/if}
      {:else if tab === 'dcr' && meta}
        {#if !changes.length}
          <div class="empty-state"><Icon name="clipboard-list" size={34} stroke={1.5} /><p>ยังไม่มีคำขอเปลี่ยนแปลงเอกสารนี้</p></div>
        {:else}
          {#if changes.length > 1}
            <div class="dcr-pick">
              {#each changes as c (c.id)}
                <button class="st-btn {shownChange?.id === c.id ? 'active' : ''}" on:click={() => (selChange = c.id)}>
                  {c.refNo} <span class="st-n">{DCR_STATUS[c.status]}</span>
                </button>
              {/each}
            </div>
          {/if}
          {#if shownChange}
            {#key shownChange.id}
              <ChangePanel change={shownChange} {doc} {me} on:changed={onChangeUpdated} />
            {/key}
          {/if}
        {/if}
      {/if}
    </div>

    {#if signing}
      <SignDialog
        title="{doc.code} · {doc.name}"
        summary={signing === 'office'
          ? 'ฉบับที่แก้ในตัวแก้ไขเอกสาร → ส่งเป็นฉบับร่างให้ผู้ตรวจสอบและผู้อนุมัติ (ฉบับปัจจุบันยังใช้งานจนกว่าจะอนุมัติ)'
          : signing === 'edit'
          ? 'แก้ไขบนเว็บ → ส่งเป็นฉบับร่างให้ผู้ตรวจสอบและผู้อนุมัติ (ฉบับปัจจุบันยังใช้งานจนกว่าจะอนุมัติ)'
          : `แนบไฟล์ “${pendingUpload?.name}” → ส่งเป็นฉบับร่างให้ผู้ตรวจสอบและผู้อนุมัติ`}
        confirmLabel="ส่งขออนุมัติ"
        {me}
        withPin
        change={{ fromRev: meta?.rev || doc.rev, newRev: meta?.nextRev || '' }}
        on:confirm={doSave}
        on:cancel={cancelSign}
      />
    {/if}

    {#if acking && myAck}
      <DecisionDialog
        title="รับทราบเอกสาร — อ่านและเข้าใจแล้ว"
        message="{doc.code} · {doc.name} · {myAck.rev}"
        {me}
        commentLabel="ความเห็น/ข้อสงสัย (ถ้ามี)"
        confirmLabel="ยืนยันรับทราบ"
        on:confirm={onAck}
        on:cancel={() => (acking = false)}
      />
    {/if}

    {#if statusAct}
      <DecisionDialog
        title={statusAct === 'obsolete' ? 'ประกาศยกเลิกใช้เอกสาร' : 'นำเอกสารกลับมาใช้'}
        message="{doc.code} · {doc.name} · {meta?.rev || doc.rev}"
        {me}
        commentLabel={statusAct === 'obsolete' ? 'เหตุผลการยกเลิกใช้' : 'เหตุผลการนำกลับมาใช้'}
        commentRequired
        confirmLabel={statusAct === 'obsolete' ? 'ยกเลิกใช้' : 'นำกลับมาใช้'}
        danger={statusAct === 'obsolete'}
        on:confirm={onStatusAct}
        on:cancel={() => (statusAct = null)}
      />
    {/if}

    {#if confirmAction}
      <ConfirmDialog
        title="ยืนยันการทำรายการ"
        message={confirmAction.message}
        confirmLabel="ดำเนินการต่อ"
        danger
        on:confirm={() => { confirmAction.run(); confirmAction = null }}
        on:cancel={() => (confirmAction = null)}
      />
    {/if}
  </div>
</div>
