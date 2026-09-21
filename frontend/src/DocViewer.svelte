<script>
  import { createEventDispatcher, onMount } from 'svelte'
  import * as XLSX from 'xlsx'
  import { api, fileUrl, b64ToBytes, bytesToB64 } from './lib/api.js'
  import SignDialog from './SignDialog.svelte'
  import ConfirmDialog from './ConfirmDialog.svelte'
  import Icon from './Icon.svelte'

  export let doc // { code, name, deptName, rev, file }
  const dispatch = createEventDispatcher()

  let meta = null, ver = null, content = null, sheet = null
  let wb = null
  let dirty = false, editing = false, tab = 'doc'
  let signing = null, pendingUpload = null
  let err = '', loading = true
  let uploadInput
  let confirmAction = null // { message, run }

  $: isLatest = meta && ver === meta.versions[0]?.version

  onMount(() => load())

  async function load(v) {
    loading = true; err = ''
    try {
      meta = await api.versions(doc.code)
      ver = v || meta.versions[0]?.version
      content = await api.content(doc.code, ver)
      dirty = false; editing = false
      if (content.kind === 'sheet') {
        wb = XLSX.read(b64ToBytes(content.data), { type: 'array' })
        pickSheet(wb.SheetNames[0])
      } else { wb = null; sheet = null }
    } catch (e) { err = 'เปิดเอกสารไม่สำเร็จ: ' + e.message }
    loading = false
  }

  function pickSheet(name) {
    const ws = wb.Sheets[name]
    const rows = XLSX.utils.sheet_to_json(ws, { header: 1, defval: '', blankrows: true, raw: false })
    const w = Math.max(1, ...rows.map((r) => r.length))
    const norm = rows.map((r) => Array.from({ length: w }, (_, i) => (r[i] == null ? '' : String(r[i]))))
    sheet = { names: wb.SheetNames, active: name, rows: norm.length ? norm : [['']] }
  }
  function switchSheet(name) {
    if (!dirty) { pickSheet(name); return }
    confirmAction = {
      message: 'สลับชีตจะไม่เก็บการแก้ไขที่ยังไม่บันทึก ดำเนินการต่อ?',
      run: () => { pickSheet(name); dirty = false },
    }
  }
  function addRow() {
    sheet.rows = [...sheet.rows, Array(sheet.rows[0].length).fill('')]; dirty = true
  }
  function buildEditedFile() {
    const ws = XLSX.utils.aoa_to_sheet(sheet.rows)
    wb.Sheets[sheet.active] = ws
    const ext = meta.ext === 'xls' ? 'xls' : 'xlsx'
    const out = XLSX.write(wb, { bookType: ext === 'xls' ? 'biff8' : 'xlsx', type: 'array' })
    return { bytes: new Uint8Array(out), ext }
  }
  function onPickFile(e) {
    const f = e.target.files?.[0]; e.target.value = ''
    if (!f) return
    if (f.size > 40 * 1024 * 1024) { err = 'ไฟล์ใหญ่เกิน 40 MB'; return }
    const rd = new FileReader()
    rd.onload = () => {
      pendingUpload = { name: f.name, ext: (f.name.split('.').pop() || '').toLowerCase(), b64: String(rd.result).split(',')[1], size: f.size }
      signing = 'upload'
    }
    rd.readAsDataURL(f)
  }
  async function doSave(e) {
    const { signerName, signerRole, note, fail } = e.detail
    try {
      let payload
      if (signing === 'edit') {
        const { bytes, ext } = buildEditedFile()
        payload = { dataBase64: bytesToB64(bytes), ext, source: 'edit', filename: doc.file.replace(/\.[^.]+$/, '') + '.' + ext, note, signerName, signerRole }
      } else {
        payload = { dataBase64: pendingUpload.b64, ext: pendingUpload.ext, source: 'upload', filename: pendingUpload.name, note, signerName, signerRole }
      }
      const { ok, body } = await api.save(doc.code, payload)
      if (!ok) { fail(body.error || 'บันทึกไม่สำเร็จ'); return }
      signing = null; pendingUpload = null
      await load(body.version.version)
      dispatch('saved')
    } catch (ex) { fail('บันทึกไม่สำเร็จ: ' + ex.message) }
  }
  function close() {
    if (!dirty) { dispatch('close'); return }
    confirmAction = {
      message: 'มีการแก้ไขที่ยังไม่ได้บันทึก ปิดหน้าต่างเลยหรือไม่?',
      run: () => dispatch('close'),
    }
  }
  const srcLabel = (s) => (s === 'original' ? 'ต้นฉบับ' : s === 'edit' ? 'แก้บนเว็บ' : 'อัปโหลด')
  const nextV = () => (meta?.versions[0]?.version || 1) + 1
</script>

<div class="mbg" on:mousedown|self={close}>
  <div class="mbox">
    <header class="mhead">
      <div class="mt">
        <b>{doc.code}</b> {doc.name}
        <div class="msub">
          {doc.deptName} · {doc.rev}
          {#if meta} · กำลังดูเวอร์ชัน {ver} จาก {meta.versions.length}{/if}
          {#if meta && !isLatest} · <b>ไม่ใช่เวอร์ชันล่าสุด</b>{/if}
        </div>
      </div>
      <button class="mx inline-flex items-center justify-center" on:click={close}><Icon name="x" size={16} /></button>
    </header>

    <nav class="mtabs">
      <button class={tab === 'doc' ? 'active' : ''} on:click={() => (tab = 'doc')}>เอกสาร</button>
      <button class={tab === 'hist' ? 'active' : ''} on:click={() => (tab = 'hist')}>ประวัติเวอร์ชัน ({meta?.versions.length || 0})</button>
      <span class="grow"></span>
      {#if !loading && tab === 'doc' && meta}
        <a class="mbtn gh inline-flex items-center gap-1.5" href={fileUrl(doc.code, ver, true)} download><Icon name="download" size={14} /> ดาวน์โหลด</a>
        <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="upload" size={14} /> อัปโหลดเวอร์ชันใหม่</button>
        <input bind:this={uploadInput} type="file" class="hidden" on:change={onPickFile} />
        {#if meta.editable && isLatest}
          {#if editing}
            <button class="mbtn accent inline-flex items-center gap-1.5" disabled={!dirty} on:click={() => (signing = 'edit')}><Icon name="save" size={14} /> บันทึกเป็นเวอร์ชันใหม่</button>
          {:else}
            <button class="mbtn inline-flex items-center gap-1.5" on:click={() => (editing = true)}><Icon name="pencil" size={14} /> แก้ไข</button>
          {/if}
        {/if}
      {/if}
    </nav>

    <div class="mbody">
      {#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={() => load(ver)}>ลองใหม่</button></div>{/if}
      {#if loading}
        <div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังเปิดเอกสาร…</p></div>
      {:else if tab === 'doc' && content}
        {#if content.kind === 'sheet' && sheet}
          {#if sheet.names.length > 1}
            <div class="sheettabs">
              {#each sheet.names as n}
                <button class={n === sheet.active ? 'active' : ''} on:click={() => switchSheet(n)}>{n}</button>
              {/each}
            </div>
          {/if}
          {#if editing}<div class="edhint inline-flex items-center gap-1.5"><Icon name="pencil" size={14} /> กำลังแก้ไข — คลิกในช่องเพื่อพิมพ์ แล้วกด “บันทึกเป็นเวอร์ชันใหม่”</div>{/if}
          <div class="sheetwrap">
            <table class="sheet"><tbody>
              {#each sheet.rows as row, ri}
                <tr>
                  <th class="rn">{ri + 1}</th>
                  {#each row as cell, ci}
                    <td>
                      {#if editing}
                        <input bind:value={sheet.rows[ri][ci]} on:input={() => (dirty = true)} />
                      {:else}<span>{cell}</span>{/if}
                    </td>
                  {/each}
                </tr>
              {/each}
            </tbody></table>
          </div>
          {#if editing}<button class="mbtn gh" on:click={addRow}>+ เพิ่มแถว</button>{/if}
        {:else if content.kind === 'html'}
          <div class="dochtml">{@html content.html}</div>
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
      {:else if tab === 'hist' && meta}
        <div class="vtree">
          {#each meta.versions as v}
            <div class="vrow {v.version === ver ? 'cur' : ''}">
              <div class="vcard">
                <div class="vtop"><span class="vn">เวอร์ชัน {v.version}</span>
                  <span class="vsrc {v.source}">{srcLabel(v.source)}</span>
                  {#if v.version === ver}<span class="mnote">· กำลังดู</span>{/if}
                </div>
                <div class="vwho inline-flex items-center gap-1.5"><Icon name="pen-line" size={14} /> <b>{v.signerName}</b>{#if v.signerRole} · {v.signerRole}{/if}</div>
                <div class="vmeta inline-flex items-center gap-1.5"><Icon name="clock" size={12} /> {new Date(v.signedAt).toLocaleString('th-TH')} · {v.filename}</div>
                {#if v.note}<div class="vnote inline-flex items-center gap-1.5"><Icon name="note" size={13} /> {v.note}</div>{/if}
                <div class="vbtns">
                  {#if v.version !== ver}<button class="mbtn gh" on:click={() => load(v.version)}>เปิดดูเวอร์ชันนี้</button>{/if}
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
        summary={signing === 'edit' ? `บันทึกการแก้ไขตารางเป็นเวอร์ชัน ${nextV()}` : `อัปโหลดไฟล์ “${pendingUpload?.name}” เป็นเวอร์ชัน ${nextV()}`}
        on:confirm={doSave}
        on:cancel={() => { signing = null; pendingUpload = null }}
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
