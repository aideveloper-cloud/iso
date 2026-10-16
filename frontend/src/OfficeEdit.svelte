<script>
  // แก้ไขเอกสารบนเว็บด้วย ONLYOFFICE (ฝังในหน้าเอกสาร) — ดู api/src/editsession.rs
  // กดบันทึกในตัวแก้ไข → ไฟล์เข้าสำเนาทำงานของรอบแก้ไข → ดู PDF ได้ → ส่งขออนุมัติ (DCR)
  import { createEventDispatcher, onMount, onDestroy, tick } from 'svelte'
  import { api, editPdfUrl } from './lib/api.js'
  import { fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let code = ''
  /** แก้ฉบับร่างของ DCR ที่ถูกตีกลับ (0 = เริ่มจากฉบับใช้งานปัจจุบัน) */
  export let changeId = 0
  /** ไฟล์ในคลังภาพรวมองค์กร (ไม่อยู่ในทะเบียน) — แทน code */
  export let ovPath = ''
  export let submitLabel = 'ส่งขออนุมัติ'
  const dispatch = createEventDispatcher()

  const holderId = `oo-${Math.random().toString(36).slice(2)}`
  let session = null
  let others = [] // คนอื่นที่เปิดแก้เอกสารนี้อยู่
  let editor = null
  let state = 'loading' // loading | editing | error
  let err = ''
  let unsaved = false // ตัวแก้ไขบอกว่ามีการแก้ที่ยังไม่บันทึก
  let view = 'edit' // edit | pdf
  let busy = false
  let timer
  let root
  let full = false // เต็มจอ (Fullscreen API ถ้ามี ไม่งั้นขยายเต็มหน้าต่าง)
  let pdfSrc = '', pdfFor = '', pdfState = 'idle', pdfErr = ''

  $: saved = session && session.saves > 0
  $: canSubmit = session && session.changed && !unsaved
  $: if (view === 'pdf' && session && saved && pdfFor !== session.sha256) loadPdf()

  onMount(async () => {
    document.addEventListener('fullscreenchange', onFsChange)
    window.addEventListener('keydown', onEsc, true)
    window.addEventListener('focus', poll)
    timer = setInterval(() => { if (!document.hidden) poll() }, 4000)
    await open()
  })
  onDestroy(() => {
    document.removeEventListener('fullscreenchange', onFsChange)
    window.removeEventListener('keydown', onEsc, true)
    if (document.fullscreenElement === root) document.exitFullscreen?.().catch(() => {})
    clearInterval(timer)
    window.removeEventListener('focus', poll)
    try { editor?.destroyEditor() } catch (e) { /* ปิดไปแล้ว */ }
    if (pdfSrc) URL.revokeObjectURL(pdfSrc)
  })

  async function toggleFull() {
    if (!full) {
      full = true
      try { await root?.requestFullscreen?.() } catch (e) { /* ใช้แบบขยายเต็มหน้าต่างแทน */ }
    } else {
      full = false
      if (document.fullscreenElement) await document.exitFullscreen().catch(() => {})
    }
  }
  // กล่องยืนยัน/ลงนามอยู่นอกตัวแก้ไข — ออกจากเต็มจอก่อน ไม่งั้นกล่องถูกบังจนมองไม่เห็น
  async function submit() {
    if (full) await toggleFull()
    dispatch('submit', { editId: session.id })
  }
  // กด Esc ออกจากเต็มจอของเบราว์เซอร์แล้ว → กลับขนาดปกติ
  function onFsChange() { if (!document.fullscreenElement && full) full = false }
  // แบบขยายเต็มหน้าต่าง (ไม่มี Fullscreen API): Esc ย่อกลับ ไม่ปิดหน้าต่างเอกสาร
  function onEsc(e) {
    if (e.key === 'Escape' && full && !document.fullscreenElement) { e.stopPropagation(); full = false }
  }

  /** โหลด api.js ของ ONLYOFFICE ครั้งเดียวต่อหน้า */
  function loadDocsApi(base) {
    if (window.DocsAPI) return Promise.resolve()
    if (!window.__isoDocsApi) {
      window.__isoDocsApi = new Promise((resolve, reject) => {
        const s = document.createElement('script')
        s.src = `${base}/web-apps/apps/api/documents/api.js`
        s.onload = () => resolve()
        s.onerror = () => { window.__isoDocsApi = null; reject(new Error('โหลดตัวแก้ไขเอกสารไม่ได้ — ตรวจว่า ONLYOFFICE บน server ทำงานอยู่')) }
        document.head.appendChild(s)
      })
    }
    return window.__isoDocsApi
  }

  async function open() {
    state = 'loading'
    err = ''
    try {
      const { ok, body } = ovPath ? await api.ovEditStart(ovPath) : await api.editStart(code, changeId)
      if (!ok) throw new Error(body.error || 'เปิดการแก้ไขไม่สำเร็จ')
      session = body.session
      others = body.others || []
      if (!body.office) throw new Error('ยังไม่ได้ตั้งค่าตัวแก้ไขเอกสาร (ONLYOFFICE) บน server นี้')
      await loadDocsApi(body.office.url)
      state = 'editing'
      view = 'edit'
      await tick()
      try { editor?.destroyEditor() } catch (e) { /* ตัวเก่า */ }
      editor = new window.DocsAPI.DocEditor(holderId, {
        ...body.office.config,
        events: {
          onDocumentStateChange: (e) => { unsaved = !!e.data },
          onError: (e) => { err = `ตัวแก้ไขแจ้งข้อผิดพลาด: ${e?.data?.errorDescription || e?.data?.errorCode || ''}` },
        },
      })
    } catch (e) {
      state = 'error'
      err = e.message
    }
  }

  async function poll() {
    if (!session || session.status !== 'open') return
    try {
      const s = await api.editStatus(session.id)
      if (s && !s.error) session = s
    } catch (e) { /* ลองรอบหน้า */ }
  }

  async function loadPdf() {
    const sha = session.sha256
    pdfFor = sha
    pdfState = 'loading'
    pdfErr = ''
    try {
      const r = await fetch(editPdfUrl(session.id, sha))
      if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || `HTTP ${r.status}`)
      const blob = await r.blob()
      if (pdfFor !== sha) return
      if (pdfSrc) URL.revokeObjectURL(pdfSrc)
      pdfSrc = URL.createObjectURL(blob)
      pdfState = 'ok'
    } catch (e) {
      if (pdfFor !== sha) return
      pdfState = 'fail'
      pdfErr = e.message
    }
  }

  async function discard() {
    if (!session) return
    busy = true
    const { ok, body } = await api.editCancel(session.id)
    busy = false
    if (!ok) { err = body.error || 'ยกเลิกไม่สำเร็จ'; return }
    dispatch('closed')
  }
</script>

<div class="oo" class:full bind:this={root}>
  <div class="oo-bar">
    <div class="oo-title">
      <Icon name="pencil" size={16} />
      <b>แก้ไขเอกสาร</b>
      {#if unsaved}
        <span class="oo-st warn">มีการแก้ที่ยังไม่บันทึก — กด Ctrl+S</span>
      {:else if saved}
        <span class="oo-st ok">บันทึกล่าสุด {fmtWhen(session.savedAt)}</span>
      {:else if state === 'editing'}
        <span class="oo-st">ยังไม่มีการแก้ไข</span>
      {/if}
    </div>
    <span class="grow"></span>
    {#if state === 'editing'}
      <div class="oo-seg">
        <button class:active={view === 'edit'} on:click={() => (view = 'edit')}>แก้ไข</button>
        <button class:active={view === 'pdf'} disabled={!saved} title={saved ? '' : 'บันทึกก่อนจึงจะดูตัวอย่างได้'} on:click={() => (view = 'pdf')}>ดูตัวอย่าง PDF</button>
      </div>
      <button class="mbtn accent inline-flex items-center gap-1.5" disabled={busy || !canSubmit}
        title={canSubmit ? '' : unsaved ? 'บันทึกการแก้ไขก่อน (Ctrl+S)' : 'ยังไม่มีการแก้ไขที่บันทึกไว้'}
        on:click={submit}>
        <Icon name="circle-check" size={14} /> {submitLabel}
      </button>
      <button class="mbtn gh" disabled={busy} on:click={discard}>ทิ้งการแก้ไข</button>
      <button class="mbtn gh inline-flex items-center gap-1.5" title={full ? 'ย่อกลับ (Esc)' : 'ขยายตัวแก้ไขเต็มจอ'} on:click={toggleFull}>
        <Icon name={full ? 'minimize' : 'maximize'} size={14} /> {full ? 'ย่อ' : 'เต็มจอ'}
      </button>
    {/if}
  </div>

  {#if others.length}
    <div class="ver-warn">
      <Icon name="alert-triangle" size={16} />
      <span><b>{others.map((o) => o.name).join(', ')}</b> กำลังแก้เอกสารนี้อยู่เช่นกัน (ล่าสุด {fmtWhen(others[0].updatedAt)})
        — แต่ละคนแก้คนละสำเนา ส่งขออนุมัติได้ครั้งละ 1 คำขอ ควรตกลงกันก่อนว่าใครเป็นผู้แก้</span>
    </div>
  {/if}
  {#if err}<div class="merr"><span>{err}</span>{#if state === 'error'}<button class="retry" on:click={open}>ลองใหม่</button>{/if}</div>{/if}

  {#if state === 'loading'}
    <div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังเปิดตัวแก้ไขเอกสาร…</p></div>
  {/if}
  <!-- ตัวแก้ไขต้องอยู่ใน DOM ตลอด (สลับไปดู PDF แล้วกลับมาไม่ต้องโหลดใหม่ / งานไม่หาย) -->
  <div class="oo-frame" class:hidden={state !== 'editing' || view !== 'edit'}><div id={holderId}></div></div>
  {#if state === 'editing' && view === 'pdf'}
    <div class="oo-pdf">
      {#if pdfState === 'ok'}
        <iframe class="docframe pdfframe" src={pdfSrc + '#navpanes=0&view=FitH'} title="ฉบับที่แก้ไข"></iframe>
      {:else if pdfState === 'fail'}
        <div class="merr">แสดงตัวอย่างไม่สำเร็จ: {pdfErr}</div>
      {:else}
        <div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังแปลงฉบับที่บันทึกเป็น PDF…</p></div>
      {/if}
    </div>
  {/if}
  {#if !full}
    <p class="mut oo-hint">กดบันทึก (Ctrl+S) ในตัวแก้ไขแล้วกด “{submitLabel}”{ovPath ? ' — เวอร์ชันเดิมยังเก็บไว้ในประวัติ' : ' — ฉบับปัจจุบันยังใช้งานจนกว่าผู้ตรวจสอบและผู้อนุมัติจะลงนาม'}</p>
  {/if}
</div>

<style>
  .oo { display: flex; flex-direction: column; gap: 8px; }
  .oo-bar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 8px 12px; border: 1px solid #e3e6ea; border-radius: 10px; background: #fff; }
  .oo-title { display: inline-flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .oo-st { font-size: 12px; color: #5b6573; background: #f1f3f5; border-radius: 999px; padding: 2px 10px; }
  .oo-st.ok { color: #1f6b3a; background: #e6f5ec; }
  .oo-st.warn { color: #8a5a00; background: #fff3d6; }
  .grow { flex: 1; }
  .oo-seg { display: inline-flex; border: 1px solid #d5dbe1; border-radius: 8px; overflow: hidden; }
  .oo-seg button { padding: 5px 12px; font-size: 13px; background: #fff; }
  .oo-seg button.active { background: #1f3f7a; color: #fff; }
  .oo-seg button:disabled { opacity: 0.5; cursor: not-allowed; }
  /* พอดีกรอบหน้าต่างเอกสาร — แท็บชีต (Excel) และแถบสถานะด้านล่างต้องเห็นโดยไม่ต้องเลื่อน */
  .oo-frame { height: calc(100vh - 320px); min-height: 400px; border: 1px solid #e3e6ea; border-radius: 10px; overflow: hidden; }
  .oo-frame.hidden { display: none; }
  .oo-pdf { min-height: 480px; }
  .oo-hint { font-size: 12px; margin: 0 4px; }
  /* เต็มจอ: ตัวแก้ไขกินพื้นที่ทั้งหมด แถบเครื่องมือของเราอยู่บนสุด */
  .oo.full { position: fixed; inset: 0; z-index: 1000; background: #f4f6f8; padding: 8px; }
  .oo.full .oo-frame, .oo.full .oo-pdf { flex: 1; height: auto; min-height: 0; }
  .oo.full .oo-pdf :global(iframe) { height: 100%; }
</style>
