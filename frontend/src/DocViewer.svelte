<script>
  import { createEventDispatcher, onMount } from 'svelte'
  import * as XLSX from 'xlsx'
  import { api, fileUrl, b64ToBytes, bytesToB64 } from './lib/api.js'
  import SignDialog from './SignDialog.svelte'

  export let doc // { code, name, deptName, rev, file }
  const dispatch = createEventDispatcher()

  let meta = null, ver = null, content = null, sheet = null
  let wb = null
  let dirty = false, editing = false, tab = 'doc'
  let signing = null, pendingUpload = null
  let err = '', loading = true
  let uploadInput

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
    if (dirty && !confirm('สลับชีตจะไม่เก็บการแก้ไขที่ยังไม่บันทึก ดำเนินการต่อ?')) return
    pickSheet(name); dirty = false
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
    if (dirty && !confirm('มีการแก้ไขที่ยังไม่ได้บันทึก ปิดหน้าต่างเลยหรือไม่?')) return
    dispatch('close')
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
      <button class="mx" on:click={close}>✕</button>
    </header>

    <nav class="mtabs">
      <button class={tab === 'doc' ? 'active' : ''} on:click={() => (tab = 'doc')}>เอกสาร</button>
      <button class={tab === 'hist' ? 'active' : ''} on:click={() => (tab = 'hist')}>ประวัติเวอร์ชัน ({meta?.versions.length || 0})</button>
      <span class="grow"></span>
      {#if !loading && tab === 'doc' && meta}
        <a class="mbtn gh" href={fileUrl(doc.code, ver, true)} download>⬇ ดาวน์โหลด</a>
        <button class="mbtn gh" on:click={() => uploadInput.click()}>⬆ อัปโหลดเวอร์ชันใหม่</button>
        <input bind:this={uploadInput} type="file" style="display:none" on:change={onPickFile} />
        {#if meta.editable && isLatest}
          {#if editing}
            <button class="mbtn" disabled={!dirty} on:click={() => (signing = 'edit')}>💾 บันทึกเป็นเวอร์ชันใหม่</button>
          {:else}
            <button class="mbtn" on:click={() => (editing = true)}>✏️ แก้ไข</button>
          {/if}
        {/if}
      {/if}
    </nav>

    <div class="mbody">
      {#if err}<div class="merr">{err}</div>{/if}
      {#if loading}
        <p class="mnote" style="text-align:center;padding:40px">กำลังเปิดเอกสาร…</p>
      {:else if tab === 'doc' && content}
        {#if content.kind === 'sheet' && sheet}
          {#if sheet.names.length > 1}
            <div class="sheettabs">
              {#each sheet.names as n}
                <button class={n === sheet.active ? 'active' : ''} on:click={() => switchSheet(n)}>{n}</button>
              {/each}
            </div>
          {/if}
          {#if editing}<div class="edhint">✏️ กำลังแก้ไข — คลิกในช่องเพื่อพิมพ์ แล้วกด “บันทึกเป็นเวอร์ชันใหม่”</div>{/if}
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
          <div class="nopreview"><div class="big">📄</div><p>{content.reason}</p>
            <a class="mbtn" href={fileUrl(doc.code, ver, true)} download>⬇ ดาวน์โหลดไฟล์</a></div>
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
                <div class="vwho">✍ <b>{v.signerName}</b>{#if v.signerRole} · {v.signerRole}{/if}</div>
                <div class="vmeta">🕒 {new Date(v.signedAt).toLocaleString('th-TH')} · {v.filename}</div>
                {#if v.note}<div class="vnote">📝 {v.note}</div>{/if}
                <div class="vbtns">
                  {#if v.version !== ver}<button class="mbtn gh" on:click={() => load(v.version)}>เปิดดูเวอร์ชันนี้</button>{/if}
                  <a class="mbtn gh" href={fileUrl(doc.code, v.version, true)} download>⬇ ดาวน์โหลด</a>
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
  </div>
</div>
