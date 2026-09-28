<script>
  import { createEventDispatcher, onMount, onDestroy, tick } from 'svelte'
  import * as XLSX from 'xlsx'
  import { htmlToDocx } from './lib/htmldocx.js'
  import { srcLabel, fmtWhen, captureEditDraft, htmlFromDraft } from './lib/changeNote.js'
  import { api, fileUrl, b64ToBytes, bytesToB64 } from './lib/api.js'
  import { sheetFromWb } from './lib/sheet.js'
  import SignDialog from './SignDialog.svelte'
  import ConfirmDialog from './ConfirmDialog.svelte'
  import ChangeHistory from './ChangeHistory.svelte'
  import RteToolbar from './RteToolbar.svelte'
  import EditActions from './EditActions.svelte'
  import Icon from './Icon.svelte'

  export let doc // { code, name, deptName, rev, file }
  const dispatch = createEventDispatcher()

  let meta = null, ver = null, content = null, sheet = null
  let wb = null
  let dirty = false, editing = false, tab = 'doc'
  let signing = null // 'edit' | 'upload' | null
  let pendingUpload = null
  let err = '', loading = true
  let uploadInput
  let confirmAction = null // { message, run }
  let editorEl
  let draftHtml = ''
  let draftSheet = null // { active, rows } snapshot ตอนกดอัปเดท
  let savedMsg = ''

  $: latestVer = meta?.versions[0]?.version
  $: isLatest = meta != null && ver === latestVer
  $: isSheetExt = meta?.ext === 'xls' || meta?.ext === 'xlsx'
  $: chronVersions = meta ? [...meta.versions].reverse() : []
  $: canWebEdit = meta?.editable && isLatest && content && content.kind !== 'download' && content.kind !== 'embed'

  function exec(e) {
    const { cmd, val } = e.detail
    document.execCommand(cmd, false, val || null)
    editorEl?.focus()
    dirty = true
  }

  function askConfirm(message, run) {
    confirmAction = { message, run }
  }

  function cancelEdit() {
    editing = false
    dirty = false
    load(ver)
  }

  async function startEdit() {
    editing = true
    dirty = false
    await tick()
    // ใส่ HTML ครั้งเดียว — ห้ามใช้ {@html} ใน contenteditable เพราะ dirty จะเรนเดอร์ทับการแก้
    if (editorEl && content?.kind === 'html') {
      editorEl.innerHTML = content.html || ''
    }
  }

  function nextV() {
    return (latestVer || 1) + 1
  }

  function isLatestVer(v) {
    return v.version === latestVer
  }

  onMount(() => {
    load()
    window.addEventListener('keydown', onKey)
  })
  onDestroy(() => window.removeEventListener('keydown', onKey))

  function onKey(e) {
    if (e.key === 'Escape' && !signing && !confirmAction) close()
  }

  async function load(v) {
    loading = true
    err = ''
    content = null
    sheet = null
    wb = null
    dirty = false
    editing = false
    try {
      meta = await api.versions(doc.code)
      ver = v || meta.versions[0]?.version
      try {
        content = await api.content(doc.code, ver)
        if (content?.error) throw new Error(content.error)
        if (content.kind === 'sheet') {
          wb = XLSX.read(b64ToBytes(content.data), { type: 'array' })
          pickSheet(wb.SheetNames[0])
        }
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

  function pickSheet(name) {
    sheet = sheetFromWb(wb, name)
  }

  function switchSheet(name) {
    if (!dirty) {
      pickSheet(name)
      return
    }
    askConfirm('สลับชีตจะไม่เก็บการแก้ไขที่ยังไม่บันทึก ดำเนินการต่อ?', () => {
      pickSheet(name)
      dirty = false
    })
  }

  function addRow() {
    sheet.rows = [...sheet.rows, Array(sheet.rows[0].length).fill('')]
    dirty = true
  }

  async function buildEditedFile() {
    if (content.kind === 'sheet') {
      const active = draftSheet?.active || sheet.active
      const rows = draftSheet?.rows || sheet.rows
      const ws = XLSX.utils.aoa_to_sheet(rows)
      wb.Sheets[active] = ws
      const ext = meta.ext === 'xls' ? 'xls' : 'xlsx'
      const out = XLSX.write(wb, { bookType: ext === 'xls' ? 'biff8' : 'xlsx', type: 'array' })
      return { bytes: new Uint8Array(out), ext }
    }
    return { bytes: htmlToDocx(htmlFromDraft(draftHtml, editorEl, content)), ext: 'docx' }
  }

  function beginUpdate() {
    const draft = captureEditDraft(content, sheet, editorEl)
    draftHtml = draft.draftHtml
    draftSheet = draft.draftSheet
    signing = 'edit'
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
    const { signerName, signerRole, note, fail } = e.detail
    try {
      let dataBase64, ext, source, filename
      if (signing === 'edit') {
        const edited = await buildEditedFile()
        dataBase64 = bytesToB64(edited.bytes)
        ext = edited.ext
        source = 'edit'
        filename = doc.file.replace(/\.[^.]+$/, '') + '.' + ext
      } else {
        dataBase64 = pendingUpload.b64
        ext = pendingUpload.ext
        source = 'upload'
        filename = pendingUpload.name
      }
      const { ok, body } = await api.save(doc.code, { dataBase64, ext, source, filename, note, signerName, signerRole })
      if (!ok) {
        fail(body.error || 'บันทึกไม่สำเร็จ')
        return
      }
      signing = null
      pendingUpload = null
      draftHtml = ''
      draftSheet = null
      const nv = body.version.version
      savedMsg = `บันทึกการแก้ไขไฟล์สำเร็จ — เวอร์ชัน ${nv} อยู่ในระบบแล้ว`
      await load(nv)
      dispatch('saved')
      setTimeout(() => { if (savedMsg.includes(`เวอร์ชัน ${nv}`)) savedMsg = '' }, 6000)
    } catch (ex) {
      fail('บันทึกไม่สำเร็จ: ' + ex.message)
    }
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
    if (meta?.editable) await startEdit()
  }

  function openVersion(v) {
    tab = 'doc'
    load(v)
  }

  function cellInput(ri, ci, e) {
    sheet.rows[ri][ci] = e.currentTarget.value
    dirty = true
  }
</script>

<div class="mbg" on:mousedown|self={close}>
  <div class="mbox wide">
    <header class="mhead">
      <div class="mt">
        <b>{doc.code}</b> {doc.name}
        {#if meta && isLatest}<span class="status-active">Active</span>{/if}
        <div class="msub">
          {doc.deptName} · {doc.rev}
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
      <span class="grow"></span>
      {#if !loading && tab === 'doc' && meta}
        {#if content}<a class="mbtn gh inline-flex items-center gap-1.5" href={fileUrl(doc.code, ver, true)} download><Icon name="download" size={14} /> ดาวน์โหลดเอกสาร</a>{/if}
        <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="cloud-upload" size={14} /> แนบไฟล์</button>
        <input bind:this={uploadInput} type="file" class="hidden" on:change={onPickFile} />
        {#if meta.editable && isLatest && content}
          <EditActions {editing} {dirty} on:cancel={cancelEdit} on:start={startEdit} on:update={beginUpdate} />
        {/if}
      {/if}
    </nav>

    <div class="mbody">
      {#if savedMsg}<div class="save-ok inline-flex items-center gap-1.5"><Icon name="circle-check" size={16} /> {savedMsg}</div>{/if}
      {#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={() => load(ver)}>ลองใหม่</button></div>{/if}
      {#if loading}
        <div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังเปิดเอกสาร…</p></div>
      {:else if tab === 'doc' && meta}
        <div class="file-tools">
          <div class="ft-head">
            <b>การแก้ไขไฟล์</b>
            <span class="mut">ดู · แก้บนเว็บ · แนบไฟล์ · บันทึกประวัติเข้าสู่ระบบ</span>
          </div>
          <div class="ft-actions">
            {#if canWebEdit}
              <EditActions {editing} {dirty} accent editLabel="แก้ไขไฟล์บนเว็บ" on:cancel={cancelEdit} on:start={startEdit} on:update={beginUpdate} />
            {:else if isLatest}
              <span class="ft-hint">ไฟล์ชนิดนี้แก้บนเว็บไม่ได้ — ใช้แนบไฟล์ใหม่แทน</span>
            {/if}
            {#if isLatest}
              <button class="mbtn inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="cloud-upload" size={14} /> แนบไฟล์</button>
            {/if}
            {#if content}<a class="mbtn gh inline-flex items-center gap-1.5" href={fileUrl(doc.code, ver, true)} download><Icon name="download" size={14} /> ดาวน์โหลดเอกสาร</a>{/if}
            <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => (tab = 'hist')}><Icon name="clock" size={14} /> ประวัติการแก้ไข ({meta.versions.length})</button>
          </div>
        </div>

        <ChangeHistory versions={meta.versions || []} currentVer={ver} onDoc />

        {#if !content}
          <div class="nopreview">
            <div class="big text-faint flex justify-center"><Icon name="alert-triangle" size={44} stroke={1.5} /></div>
            <p>{err || 'ไม่พบไฟล์ในระบบ'}</p>
            <button class="mbtn inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="upload" size={14} /> อัปโหลดไฟล์ใหม่เข้าสู่ระบบ</button>
          </div>
        {:else}
          {#if !isLatest}
            <div class="ver-warn">
              <Icon name="alert-triangle" size={16} />
              <span>กำลังดูเวอร์ชันเก่า (v{ver}) ซึ่งไม่ควรนำไปใช้งานจริง — เวอร์ชันล่าสุดคือ <b>v{latestVer}</b></span>
              <button class="vw-btn" on:click={() => load(latestVer)}>ไปเวอร์ชันล่าสุด</button>
            </div>
          {/if}
          {#if content.kind === 'sheet' && sheet}
            {#if sheet.names.length > 1}
              <div class="sheettabs">
                {#each sheet.names as n}
                  <button class={n === sheet.active ? 'active' : ''} on:click={() => switchSheet(n)}>{n}</button>
                {/each}
              </div>
            {/if}
            {#if editing}<div class="edhint inline-flex items-center gap-1.5"><Icon name="pencil" size={14} /> กำลังแก้ไข — คลิกในช่องเพื่อพิมพ์ แล้วกด “อัปเดท”</div>{/if}
            <div class="sheetwrap">
              <table class="sheet"><tbody>
                {#each sheet.rows as row, ri}
                  <tr>
                    <th class="rn">{ri + 1}</th>
                    {#each row as cell, ci}
                      <td>
                        {#if editing}
                          <input value={sheet.rows[ri][ci]} on:input={(e) => cellInput(ri, ci, e)} />
                        {:else}<span>{cell}</span>{/if}
                      </td>
                    {/each}
                  </tr>
                {/each}
              </tbody></table>
            </div>
            {#if editing}<button class="mbtn gh" on:click={addRow}>+ เพิ่มแถว</button>{/if}
          {:else if content.kind === 'html'}
            {#if editing}
              <div class="edhint inline-flex items-center gap-1.5"><Icon name="pencil" size={14} /> กำลังแก้ไขไฟล์ — แก้ข้อความ/ตารางได้ แล้วกด “อัปเดทเข้าสู่ระบบ” เพื่อบันทึกประวัติ</div>
              <RteToolbar on:exec={exec} />
              <div class="dochtml rte" contenteditable="true" bind:this={editorEl} on:input={() => (dirty = true)}></div>
            {:else}
              <div class="dochtml">{@html content.html}</div>
            {/if}
          {:else if content.kind === 'embed'}
            {#if content.ext === 'pdf'}
              <iframe class="docframe" src={fileUrl(doc.code, ver)} title={doc.name}></iframe>
            {:else}
              <img class="docimg" src={fileUrl(doc.code, ver)} alt={doc.name} />
            {/if}
          {:else}
            <div class="nopreview"><div class="big text-faint flex justify-center"><Icon name="file-text" size={44} stroke={1.5} /></div><p>{content.reason}</p>
              <a class="mbtn inline-flex items-center gap-1.5" href={fileUrl(doc.code, ver, true)} download><Icon name="download" size={14} /> ดาวน์โหลดไฟล์</a></div>
          {/if}
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

        <ChangeHistory versions={meta.versions} currentVer={ver} showSigner>
          <svelte:fragment slot="actions" let:v>
            {#if v.version !== ver}
              <button class="mbtn gh" on:click={() => openVersion(v.version)}>เปิดดู</button>
            {/if}
            {#if isLatestVer(v)}
              {#if meta.editable}
                <button class="mbtn accent" on:click={() => openAndEdit(v.version)}>แก้ไขไฟล์</button>
              {/if}
              <button class="mbtn" on:click={() => uploadInput.click()}>แนบไฟล์</button>
            {/if}
            <a class="mbtn gh" href={fileUrl(doc.code, v.version, true)} download>ดาวน์โหลด</a>
          </svelte:fragment>
        </ChangeHistory>

        <div class="vtree">
          {#each meta.versions as v}
            <div class="vrow {v.version === ver ? 'cur' : ''}">
              <div class="vcard">
                <div class="vtop">
                  <span class="vn">Ver {v.version}</span>
                  <span class="vsrc {v.source}">{srcLabel(v.source)}</span>
                  {#if isLatestVer(v)}<span class="ver-badge latest inline-flex items-center gap-1"><Icon name="circle-check" size={12} /> แก้ล่าสุด</span>{/if}
                  {#if v.version === ver}<span class="mnote">· กำลังดู</span>{/if}
                </div>
                <div class="vwho inline-flex items-center gap-1.5"><Icon name="pen-line" size={14} /> <b>{v.signerName}</b>{#if v.signerRole} · {v.signerRole}{/if}</div>
                <div class="vmeta inline-flex items-center gap-1.5"><Icon name="clock" size={12} /> {fmtWhen(v.signedAt)} · {v.filename}</div>
                {#if v.note}<div class="vnote inline-flex items-center gap-1.5"><Icon name="note" size={13} /> {v.note}</div>{/if}
                <div class="vbtns">
                  {#if v.version !== ver}
                    <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => openVersion(v.version)}><Icon name="eye" size={14} /> เปิดดู</button>
                  {/if}
                  {#if isLatestVer(v)}
                    {#if meta.editable}
                      <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => openAndEdit(v.version)}>
                        <Icon name="pencil" size={14} /> {isSheetExt ? 'อัปเดทตาราง' : 'แก้ไขเอกสาร'}
                      </button>
                    {/if}
                    <button class="mbtn inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="cloud-upload" size={14} /> แนบไฟล์</button>
                  {:else}
                    <span class="mnote">ข้อมูลเก่า — ใช้เพื่ออ้างอิงเท่านั้น</span>
                  {/if}
                  <a class="mbtn gh inline-flex items-center gap-1.5" href={fileUrl(doc.code, v.version, true)} download><Icon name="download" size={14} /> ดาวน์โหลด</a>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    {#if signing}
      <SignDialog
        title="{doc.code} · {doc.name}"
        summary={signing === 'edit'
          ? `แก้ไขไฟล์บนเว็บ → บันทึกเป็นเวอร์ชัน ${nextV()} พร้อมประวัติการเปลี่ยนแปลง`
          : `แนบไฟล์ “${pendingUpload?.name}” → บันทึกเป็นเวอร์ชัน ${nextV()} พร้อมประวัติการเปลี่ยนแปลง`}
        confirmLabel="อัปเดทในระบบ"
        fullChange={true}
        on:confirm={doSave}
        on:cancel={cancelSign}
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
