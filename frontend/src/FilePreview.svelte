<script>
  import { createEventDispatcher, onMount, onDestroy, tick } from 'svelte'
  import * as XLSX from 'xlsx'
  import { htmlToDocx } from './lib/htmldocx.js'
  import { captureEditDraft, htmlFromDraft } from './lib/changeNote.js'
  import { api, ovFileUrl, b64ToBytes, bytesToB64 } from './lib/api.js'
  import { fileExt, fileKind } from './lib/const.js'
  import { sheetFromWb } from './lib/sheet.js'
  import SignDialog from './SignDialog.svelte'
  import ConfirmDialog from './ConfirmDialog.svelte'
  import ChangeHistory from './ChangeHistory.svelte'
  import RteToolbar from './RteToolbar.svelte'
  import EditActions from './EditActions.svelte'
  import Icon from './Icon.svelte'

  export let file // { name, path, size? }
  const dispatch = createEventDispatcher()

  let content = null, sheet = null, wb = null
  let versions = []
  let err = '', loading = true
  let dirty = false, editing = false, signing = false
  let editorEl
  let confirmAction = null // { message, run }
  let draftHtml = ''
  let draftSheet = null
  let savedMsg = ''
  let tab = 'doc'

  $: canEdit = content?.editable && (content.kind === 'html' || content.kind === 'sheet')
  $: latestVer = versions[0]?.version

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
    load()
  }

  async function startEdit() {
    editing = true
    dirty = false
    await tick()
    if (editorEl && content?.kind === 'html') {
      editorEl.innerHTML = content.html || ''
    }
  }

  function nextV() {
    return (latestVer || content?.version || 1) + 1
  }

  onMount(() => {
    load()
    window.addEventListener('keydown', onKey)
  })
  onDestroy(() => window.removeEventListener('keydown', onKey))

  function onKey(e) {
    if (e.key === 'Escape' && !signing && !confirmAction) close()
  }

  async function loadVersions(code) {
    if (!code) {
      versions = []
      return
    }
    try {
      const meta = await api.versions(code)
      versions = meta?.versions || []
    } catch {
      versions = []
    }
  }

  async function load() {
    loading = true
    err = ''
    content = null
    sheet = null
    dirty = false
    editing = false
    try {
      content = await api.overviewContent(file.path)
      if (content.error) throw new Error(content.error)
      if (content.kind === 'sheet') {
        wb = XLSX.read(b64ToBytes(content.data), { type: 'array' })
        pickSheet(wb.SheetNames[0])
      }
      await loadVersions(content.docCode)
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
      const e = (content.ext || fileExt(file)).toLowerCase() === 'xls' ? 'xls' : 'xlsx'
      const out = XLSX.write(wb, { bookType: e === 'xls' ? 'biff8' : 'xlsx', type: 'array' })
      return new Uint8Array(out)
    }
    return htmlToDocx(htmlFromDraft(draftHtml, editorEl, content))
  }

  function beginUpdate() {
    const draft = captureEditDraft(content, sheet, editorEl)
    draftHtml = draft.draftHtml
    draftSheet = draft.draftSheet
    signing = true
  }

  async function doSave(e) {
    const { signerName, signerRole, note, fail } = e.detail
    try {
      const bytes = await buildEditedFile()
      const { ok, body } = await api.overviewSave({
        path: file.path,
        dataBase64: bytesToB64(bytes),
        signerName,
        signerRole,
        note,
      })
      if (!ok) {
        fail(body.error || 'อัปเดทไม่สำเร็จ')
        return
      }
      signing = false
      draftHtml = ''
      draftSheet = null
      savedMsg = body.latestVersion
        ? `บันทึกการแก้ไขไฟล์สำเร็จ — เวอร์ชัน ${body.latestVersion} อยู่ในระบบแล้ว`
        : 'บันทึกการแก้ไขไฟล์สำเร็จ'
      await load()
      dispatch('saved', body)
      setTimeout(() => { savedMsg = '' }, 6000)
    } catch (ex) {
      fail('อัปเดทไม่สำเร็จ: ' + ex.message)
    }
  }

  function close() {
    if (!dirty) {
      dispatch('close')
      return
    }
    askConfirm('มีการแก้ไขที่ยังไม่ได้บันทึก ปิดหน้าต่างเลยหรือไม่?', () => dispatch('close'))
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
        <b>{file.name}</b>
        {#if content?.version}<span class="status-active">Active</span>{/if}
        <div class="msub">
          {fileKind(fileExt(file)).label}{#if file.size} · {(file.size / 1024).toFixed(0)} KB{/if}
          {#if content?.version}<span class="ver-badge latest inline-flex items-center gap-1 ml-1"><Icon name="circle-check" size={12} /> ในระบบ v{content.version}</span>{/if}
          {#if editing}<span class="ver-badge latest inline-flex items-center gap-1 ml-1"><Icon name="pencil" size={12} /> กำลังแก้ไข</span>{/if}
        </div>
      </div>
      <button class="mx inline-flex items-center justify-center" on:click={close}><Icon name="x" size={16} /></button>
    </header>

    <nav class="mtabs">
      <button class={tab === 'doc' ? 'active' : ''} on:click={() => (tab = 'doc')}>เอกสาร</button>
      {#if versions.length}
        <button class={tab === 'hist' ? 'active' : ''} on:click={() => (tab = 'hist')}>ประวัติการแก้ไข ({versions.length})</button>
      {/if}
      <span class="grow"></span>
      {#if !loading && tab === 'doc'}
        <a class="mbtn gh inline-flex items-center gap-1.5" href={ovFileUrl(file.path, true)} download={file.name}><Icon name="download" size={14} /> ดาวน์โหลดเอกสาร</a>
        {#if canEdit}
          <EditActions {editing} {dirty} on:cancel={cancelEdit} on:start={startEdit} on:update={beginUpdate} />
        {/if}
      {/if}
    </nav>

    <div class="mbody">
      {#if savedMsg}<div class="save-ok inline-flex items-center gap-1.5"><Icon name="circle-check" size={16} /> {savedMsg}</div>{/if}
      {#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}
      {#if loading}
        <div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังเปิดเอกสาร…</p></div>
      {:else if tab === 'hist' && versions.length}
        <ChangeHistory {versions} showSigner />
      {:else if content}
        <div class="file-tools">
          <div class="ft-head">
            <b>การแก้ไขไฟล์</b>
            <span class="mut">ดู · แก้บนเว็บ · อัปเดทเข้าสู่ระบบ · บันทึกประวัติ</span>
          </div>
          <div class="ft-actions">
            {#if canEdit}
              <EditActions {editing} {dirty} accent editLabel="แก้ไขไฟล์บนเว็บ" on:cancel={cancelEdit} on:start={startEdit} on:update={beginUpdate} />
            {:else}
              <span class="ft-hint">ไฟล์ชนิดนี้แก้บนเว็บไม่ได้ — ดาวน์โหลดแล้วอัปโหลดผ่านทะเบียนหากต้องการเปลี่ยน</span>
            {/if}
            <a class="mbtn gh inline-flex items-center gap-1.5" href={ovFileUrl(file.path, true)} download={file.name}><Icon name="download" size={14} /> ดาวน์โหลดเอกสาร</a>
            {#if versions.length}
              <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => (tab = 'hist')}><Icon name="clock" size={14} /> ประวัติการแก้ไข ({versions.length})</button>
            {/if}
          </div>
        </div>

        <ChangeHistory {versions} onDoc />

        {#if content.kind === 'sheet' && sheet}
          {#if sheet.names.length > 1}
            <div class="sheettabs">
              {#each sheet.names as n}
                <button class={n === sheet.active ? 'active' : ''} on:click={() => switchSheet(n)}>{n}</button>
              {/each}
            </div>
          {/if}
          {#if editing}<div class="edhint inline-flex items-center gap-1.5"><Icon name="pencil" size={14} /> กำลังแก้ไข — พิมพ์ในช่องแล้วกด “อัปเดทเข้าสู่ระบบ”</div>{/if}
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
            <iframe class="docframe" src={ovFileUrl(file.path)} title={file.name}></iframe>
          {:else}
            <img class="docimg" src={ovFileUrl(file.path)} alt={file.name} />
          {/if}
          <p class="mnote mt-3">ไฟล์ PDF/รูปเปิดดูได้อย่างเดียว — อัปโหลดไฟล์ใหม่ผ่านทะเบียนเอกสารหากต้องการเปลี่ยน</p>
        {:else}
          <div class="nopreview">
            <div class="big text-faint flex justify-center"><Icon name="file-text" size={44} stroke={1.5} /></div>
            <p>{content.reason || 'เปิดดูบนหน้าเว็บไม่ได้'}</p>
            <a class="mbtn inline-flex items-center gap-1.5" href={ovFileUrl(file.path, true)} download={file.name}><Icon name="download" size={14} /> ดาวน์โหลดไฟล์</a>
          </div>
        {/if}
      {/if}
    </div>

    {#if signing}
      <SignDialog
        title={file.name}
        summary={`แก้ไขไฟล์บนเว็บ → บันทึกเป็นเวอร์ชัน ${nextV()} พร้อมประวัติการเปลี่ยนแปลง`}
        confirmLabel="อัปเดทในระบบ"
        fullChange={true}
        on:confirm={doSave}
        on:cancel={() => (signing = false)}
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
