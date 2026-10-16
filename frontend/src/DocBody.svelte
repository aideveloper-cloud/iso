<script>
  // เนื้อหาเอกสารที่ดู/แก้บนเว็บ — ใช้ร่วมกันระหว่าง DocViewer (ทะเบียน) และ FilePreview (คลังภาพรวม)
  //   Word  : แสดงตามรูปแบบเดิม แล้วแก้ย่อหน้า/แถวที่เปลี่ยนลงไฟล์ .docx ต้นฉบับ (lib/docxpatch.js)
  //   Excel : แสดงตามรูปแบบเดิม (merge, สี, เส้น, ความกว้าง) แล้วแก้เฉพาะเซลล์ที่เปลี่ยนในไฟล์ต้นฉบับ (lib/xlsxpatch.js)
  import { tick, onDestroy } from 'svelte'
  import * as XLSX from 'xlsx'
  import { b64ToBytes } from './lib/api.js'
  import { loadXlsxModel } from './lib/xlsxview.js'
  import { patchXlsx } from './lib/xlsxpatch.js'
  import { makeBaseline, patchDocx, insertRowBelow, deleteRow } from './lib/docxpatch.js'
  import SheetGrid from './SheetGrid.svelte'
  import RteToolbar from './RteToolbar.svelte'
  import Icon from './Icon.svelte'

  export let content // ผลจาก /content: { kind: 'sheet'|'html'|'embed'|'download', ... }
  export let name = ''
  export let origUrl = '' // URL ไฟล์ต้นฉบับ (ใช้ดึง .docx มา patch)
  export let embedUrl = ''
  export let downloadUrl = ''
  /** URL ไฟล์ PDF ที่แปลงจากต้นฉบับ (Word/Excel) — หน้าอ่านแสดง PDF ให้เหมือนต้นฉบับ 100% */
  export let pdfUrl = ''
  export let editing = false
  export let dirty = false
  /** ชื่อปุ่มบันทึกที่อ้างถึงในข้อความแนะนำ */
  export let saveLabel = 'อัปเดทเข้าสู่ระบบ'

  let editorEl
  let prep = false
  let docErr = ''
  let note = ''
  let origBytes = null
  let baseline = null

  // ---------- PDF (หน้าอ่าน) ----------
  // ไม่ได้แก้ไขอยู่ + ไฟล์แปลงเป็น PDF ได้ → แสดง PDF; แปลงไม่สำเร็จ → กลับไปแสดงแบบ HTML/ตารางเดิม
  let pdfSrc = ''
  let pdfState = 'idle' // idle | loading | ok | fail
  let pdfErr = ''
  let pdfFor = ''
  $: wantPdf = !editing && !!content?.pdf && !!pdfUrl
  $: if (wantPdf && pdfFor !== pdfUrl) loadPdf(pdfUrl)

  async function loadPdf(u) {
    pdfFor = u
    pdfState = 'loading'
    pdfErr = ''
    try {
      const r = await fetch(u)
      if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || `HTTP ${r.status}`)
      const blob = await r.blob()
      if (pdfFor !== u) return
      if (pdfSrc) URL.revokeObjectURL(pdfSrc)
      pdfSrc = URL.createObjectURL(blob)
      pdfState = 'ok'
    } catch (e) {
      if (pdfFor !== u) return
      pdfState = 'fail'
      pdfErr = e.message
    }
  }
  onDestroy(() => { if (pdfSrc) URL.revokeObjectURL(pdfSrc) })

  // ---------- Excel ----------
  let sheets = []
  let active = 0
  let visited = {}
  let grids = []
  let xlsxBytes = null
  let xlsWb = null
  let sheetErr = ''

  $: if (content?.kind === 'sheet') initSheets(content)

  function initSheets(c) {
    sheetErr = ''
    sheets = []
    xlsWb = null
    try {
      let bytes = b64ToBytes(c.data)
      if ((c.ext || '').toLowerCase() === 'xls') {
        // .xls เป็นไบนารีเก่า แพตช์ในที่เดิมไม่ได้ — อ่านผ่าน SheetJS แล้วเขียนกลับเป็น .xls
        xlsWb = XLSX.read(bytes, { type: 'array', cellStyles: true }) // cellStyles: เก็บความกว้างคอลัมน์/ความสูงแถว
        bytes = new Uint8Array(XLSX.write(xlsWb, { bookType: 'xlsx', type: 'array', cellStyles: true }))
      }
      xlsxBytes = bytes
      const m = loadXlsxModel(bytes)
      // .xls: SheetJS ไม่เก็บรูปแบบการตัดบรรทัด → ให้ทุกเซลล์ตัดบรรทัดได้ หัวตารางยาว ๆ จะไม่ถูกตัดหาย
      if (xlsWb) for (const sh of m.sheets) for (const row of sh.cells) for (const cl of row) {
        if (!cl?.css) continue
        cl.css = cl.css.replace(/white-space:nowrap(;overflow:visible)?/, 'white-space:pre-wrap;word-break:break-word')
          // ฟอนต์จริงของไฟล์ (มักเป็น Angsana New ตัวเล็ก) ไม่ถูกเก็บ → ใช้ขนาดใกล้เคียงแทนฟอนต์มาตรฐานตัวใหญ่
          .replace(/font-size:[^;]+;?/, '').replace(/font-family:[^;]+;?/, '')
          // เส้นขอบจริงของ .xls อ่านไม่ได้ → ใช้เส้นตารางเข้มขึ้นให้เห็นโครงตาราง · จัดกึ่งกลางแนวตั้งแบบหัวตารางส่วนใหญ่
          .replaceAll('1px solid #e5e7eb', '1px solid #b9c3d1')
          .replace(/vertical-align:[^;]+/, 'vertical-align:middle') + ';font-size:10.5pt;font-family:Tahoma,Sarabun,sans-serif'
      }
      sheets = m.sheets.filter((s) => !s.hidden)
      if (!sheets.length) sheets = m.sheets
      active = 0
      visited = { 0: true }
      grids = []
    } catch (e) {
      sheetErr = 'อ่านไฟล์ตารางไม่สำเร็จ: ' + e.message
    }
  }

  function pickSheet(i) {
    active = i
    visited = { ...visited, [i]: true }
  }

  function addRow() {
    const s = sheets[active]
    if (!s || s.nRows >= 300) return
    s.cells.push(Array(s.nCols).fill(null))
    s.rowHeights.push(null)
    s.nRows++
    sheets = sheets
    dirty = true
  }

  function onCell(e) {
    if (e.detail.changed) dirty = true
  }

  function applyToXls(edits) {
    for (const ed of edits) {
      const ws = xlsWb.Sheets[ed.sheet]
      if (!ws) continue
      const addr = XLSX.utils.encode_cell({ r: ed.r, c: ed.c })
      const old = ws[addr]
      const num = ed.value.trim() !== '' && Number.isFinite(Number(ed.value.replace(/,/g, '')))
      if (ed.value === '') delete ws[addr]
      else if (num && (!old || old.t === 'n')) ws[addr] = { t: 'n', v: Number(ed.value.replace(/,/g, '')), z: old?.z }
      else ws[addr] = { t: 's', v: ed.value }
      const ref = XLSX.utils.decode_range(ws['!ref'] || 'A1')
      ref.e.r = Math.max(ref.e.r, ed.r)
      ref.e.c = Math.max(ref.e.c, ed.c)
      ws['!ref'] = XLSX.utils.encode_range(ref)
    }
  }

  // ---------- Word ----------
  async function enterEdit() {
    note = ''
    if (content?.kind !== 'html') return
    prep = true
    docErr = ''
    try {
      const r = await fetch(origUrl)
      if (!r.ok) throw new Error(`ดึงไฟล์ต้นฉบับไม่สำเร็จ (${r.status})`)
      origBytes = new Uint8Array(await r.arrayBuffer())
      await tick()
      editorEl.innerHTML = content.html || ''
      baseline = makeBaseline(editorEl)
    } catch (e) {
      docErr = 'เริ่มแก้ไขไม่ได้: ' + e.message
    } finally {
      prep = false
    }
  }

  let wasEditing = false
  $: if (editing !== wasEditing) {
    wasEditing = editing
    if (editing) enterEdit()
    else {
      origBytes = null
      baseline = null
    }
  }

  function exec(e) {
    document.execCommand(e.detail.cmd, false, null)
    editorEl?.focus()
    dirty = true
  }

  function rowOp(e) {
    note = (e.detail === 'add' ? insertRowBelow(editorEl) : deleteRow(editorEl)) || ''
    if (!note) dirty = true
  }

  function onPaste(e) {
    e.preventDefault()
    const t = (e.clipboardData || window.clipboardData).getData('text/plain')
    document.execCommand('insertText', false, t)
  }

  // ย่อหน้ากว้างกว่าหน้าต่าง -> ย่อทั้งหน้าให้พอดี
  function fitPages(node) {
    const apply = () => {
      const page = node.querySelector('.docx-page')
      if (!page) return
      const natural = parseFloat(page.style.width) || page.offsetWidth
      const avail = node.clientWidth - 32
      page.style.zoom = natural > avail && avail > 100 ? String(avail / natural) : ''
    }
    const ro = new ResizeObserver(apply)
    ro.observe(node)
    const mo = new MutationObserver(apply)
    mo.observe(node, { childList: true })
    apply()
    return { destroy: () => { ro.disconnect(); mo.disconnect() } }
  }

  /** สร้างไฟล์ที่แก้แล้ว (เรียกก่อนเปิดหน้าลงชื่อ) */
  export async function buildEdited() {
    if (content.kind === 'sheet') {
      const edits = []
      sheets.forEach((s, i) => {
        for (const e of grids[i]?.getEdits() || []) edits.push({ sheet: s.name, ...e })
      })
      if (xlsWb) {
        applyToXls(edits)
        return { bytes: new Uint8Array(XLSX.write(xlsWb, { bookType: 'biff8', type: 'array' })), ext: 'xls' }
      }
      return { bytes: patchXlsx(xlsxBytes, edits), ext: 'xlsx' }
    }
    if (!origBytes || !baseline) throw new Error('ยังไม่ได้โหลดไฟล์ต้นฉบับ')
    return { bytes: patchDocx(origBytes, editorEl, baseline), ext: 'docx' }
  }
</script>

{#if wantPdf && pdfState !== 'fail'}
  {#if pdfState === 'ok'}
    <iframe class="docframe pdfframe" src={pdfSrc + '#navpanes=0&view=FitH'} title={name}></iframe>
  {:else}
    <div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังเตรียมเอกสารให้เหมือนต้นฉบับ…</p></div>
  {/if}
{:else if content.kind === 'sheet'}
  {#if wantPdf && pdfState === 'fail'}<div class="mnote mb-2 inline-flex items-center gap-1.5"><Icon name="alert-triangle" size={13} /> แสดงแบบย่อ — แปลงเป็น PDF ไม่สำเร็จ ({pdfErr}) · ดาวน์โหลดไฟล์เพื่อดูฉบับสมบูรณ์</div>{/if}
  {#if sheetErr}
    <div class="merr"><span>{sheetErr}</span></div>
  {:else}
    {#if sheets.length > 1}
      <div class="sheettabs">
        {#each sheets as s, i}
          <button class={i === active ? 'active' : ''} on:click={() => pickSheet(i)}>{s.name}</button>
        {/each}
      </div>
    {/if}
    {#if editing}<div class="edhint inline-flex items-center gap-1.5"><Icon name="pencil" size={14} /> กำลังแก้ไข — คลิกในช่องเพื่อพิมพ์ (ช่องที่แก้จะมีสีเหลืองอ่อน) แล้วกด “{saveLabel}”{#if xlsWb} · ไฟล์ .xls เก่าอาจสูญเสียสี/เส้นขอบบางส่วนเมื่อบันทึก{/if}</div>{/if}
    {#each sheets as s, i}
      {#if visited[i]}
        <div class="sheetwrap" class:hidden={i !== active}>
          <SheetGrid model={s} {editing} bind:this={grids[i]} on:change={onCell} />
        </div>
      {/if}
    {/each}
    {#if editing && sheets[active]}<button class="mbtn gh mt-2" on:click={addRow}>+ เพิ่มแถว</button>{/if}
  {/if}
{:else if content.kind === 'html'}
  {#if editing}
    <div class="edhint inline-flex items-center gap-1.5"><Icon name="pencil" size={14} /> กำลังแก้ไขไฟล์ — แก้ข้อความ/ตารางได้ รูปแบบหน้าเอกสารเดิมจะคงไว้ แล้วกด “{saveLabel}”</div>
    <RteToolbar on:exec={exec} on:row={rowOp} />
    {#if note}<div class="mnote mb-2">{note}</div>{/if}
    {#if docErr}<div class="merr"><span>{docErr}</span></div>{/if}
    {#if prep}<div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังเตรียมไฟล์ต้นฉบับ…</p></div>{/if}
    <div
      class="dochtml rte"
      class:hidden={prep || !!docErr}
      contenteditable="true"
      bind:this={editorEl}
      use:fitPages
      on:input={() => (dirty = true)}
      on:paste={onPaste}
    ></div>
  {:else}
    {#if wantPdf && pdfState === 'fail'}<div class="mnote mb-2 inline-flex items-center gap-1.5"><Icon name="alert-triangle" size={13} /> แสดงแบบย่อ — แปลงเป็น PDF ไม่สำเร็จ ({pdfErr}) · ดาวน์โหลดไฟล์เพื่อดูฉบับสมบูรณ์</div>{/if}
    <div class="dochtml" use:fitPages>{@html content.html}</div>
  {/if}
{:else if content.kind === 'embed'}
  {#if content.ext === 'pdf'}
    <iframe class="docframe" src={embedUrl} title={name}></iframe>
  {:else}
    <img class="docimg" src={embedUrl} alt={name} />
  {/if}
{:else}
  <div class="nopreview">
    <div class="big text-faint flex justify-center"><Icon name="file-text" size={44} stroke={1.5} /></div>
    <p>{content.reason || 'เปิดดูบนหน้าเว็บไม่ได้'}</p>
    <a class="mbtn inline-flex items-center gap-1.5" href={downloadUrl} download={name}><Icon name="download" size={14} /> ดาวน์โหลดไฟล์</a>
  </div>
{/if}
