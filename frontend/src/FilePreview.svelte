<script>
  // ไฟล์ในคลังภาพรวมองค์กร (ไม่อยู่ในทะเบียน) — ดู · แก้บนเว็บ · อัปโหลดไฟล์ใหม่ ทุกครั้งเป็นเวอร์ชันใหม่ ไม่ทับของเดิม
  //   เก็บผู้แก้ไข วันเวลา ชื่อไฟล์ และย้อนดู/ดาวน์โหลดเวอร์ชันเก่าได้
  import { createEventDispatcher, onMount, onDestroy } from 'svelte'
  import { api, ovFileUrl, bytesToB64 } from './lib/api.js'
  import { fileExt, fileKind } from './lib/const.js'
  import { srcLabel, fmtWhen } from './lib/changeNote.js'
  import DocBody from './DocBody.svelte'
  import SignDialog from './SignDialog.svelte'
  import ConfirmDialog from './ConfirmDialog.svelte'
  import ChangeHistory from './ChangeHistory.svelte'
  import EditActions from './EditActions.svelte'
  import Icon from './Icon.svelte'

  export let file // { name, path, size? }
  export let me = null
  const dispatch = createEventDispatcher()

  let content = null
  let versions = []
  let ver = null // เวอร์ชันที่กำลังดู (null = ล่าสุด)
  let err = '', loading = true
  let dirty = false, editing = false
  let signing = null // 'edit' | 'upload' | null
  let pendingUpload = null
  let uploadInput
  let body // DocBody
  let confirmAction = null // { message, run }
  let draft = null // { bytes, ext }
  let savedMsg = ''
  let tab = 'doc'

  $: latestVer = versions[0]?.version
  $: isLatest = !ver || ver === latestVer
  $: writer = me && me.role !== 'viewer'
  $: canEdit = writer && isLatest && content?.editable && (content.kind === 'html' || content.kind === 'sheet')
  $: shownVer = ver || content?.version || null
  $: dl = (v) => ovFileUrl(file.path, true) + (v ? `&v=${v}` : '')
  $: view = (v) => ovFileUrl(file.path) + (v ? `&v=${v}` : '')

  function askConfirm(message, run) { confirmAction = { message, run } }
  function cancelEdit() { editing = false; dirty = false; load(ver) }
  function startEdit() { editing = true; dirty = false }
  const nextV = () => (latestVer || content?.version || 1) + 1

  onMount(() => { load(); window.addEventListener('keydown', onKey) })
  onDestroy(() => window.removeEventListener('keydown', onKey))
  function onKey(e) { if (e.key === 'Escape' && !signing && !confirmAction) close() }

  async function load(v = null) {
    loading = true; err = ''; content = null; dirty = false; editing = false
    try {
      const h = await api.overviewHistory(file.path)
      versions = h?.versions || []
      ver = v
      content = await api.overviewContent(file.path, v)
      if (content.error) throw new Error(content.error)
    } catch (e) {
      err = 'เปิดเอกสารไม่สำเร็จ: ' + e.message
    }
    loading = false
  }

  async function beginUpdate() {
    err = ''
    try { draft = await body.buildEdited(); signing = 'edit' } catch (e) { err = 'สร้างไฟล์ที่แก้ไขไม่สำเร็จ: ' + e.message }
  }
  function onPickFile(e) {
    const f = e.target.files?.[0]
    e.target.value = ''
    if (!f) return
    if (f.size > 40 * 1024 * 1024) { err = 'ไฟล์ใหญ่เกิน 40 MB'; return }
    const rd = new FileReader()
    rd.onload = () => { pendingUpload = { name: f.name, b64: String(rd.result).split(',')[1], size: f.size }; signing = 'upload' }
    rd.readAsDataURL(f)
  }

  async function doSave(e) {
    const { note, fail } = e.detail
    try {
      const payload = signing === 'edit'
        ? { path: file.path, dataBase64: bytesToB64(draft.bytes), note }
        : { path: file.path, dataBase64: pendingUpload.b64, filename: pendingUpload.name, note }
      const { ok, body } = await api.overviewSave(payload)
      if (!ok) { fail(body.error || 'บันทึกไม่สำเร็จ'); return }
      const how = signing === 'edit' ? 'บันทึกการแก้ไข' : `อัปโหลด “${pendingUpload.name}”`
      signing = null; draft = null; pendingUpload = null
      savedMsg = `${how}สำเร็จ — เป็นเวอร์ชัน ${body.latestVersion} (เวอร์ชันก่อนหน้ายังเก็บไว้ครบ)`
      await load()
      dispatch('saved', body)
      setTimeout(() => { savedMsg = '' }, 8000)
    } catch (ex) { fail('บันทึกไม่สำเร็จ: ' + ex.message) }
  }

  function close() {
    if (!dirty) { dispatch('close'); return }
    askConfirm('มีการแก้ไขที่ยังไม่ได้บันทึก ปิดหน้าต่างเลยหรือไม่?', () => dispatch('close'))
  }
  function openVersion(v) { tab = 'doc'; load(v === latestVer ? null : v) }
</script>

<div class="mbg" on:mousedown|self={close}>
  <div class="mbox wide">
    <header class="mhead">
      <div class="mt">
        <b>{file.name}</b>
        {#if content?.version && isLatest}<span class="status-active">Active</span>{/if}
        <div class="msub">
          {fileKind(fileExt(file)).label}{#if file.size} · {(file.size / 1024).toFixed(0)} KB{/if}
          {#if shownVer}
            {#if isLatest}<span class="ver-badge latest inline-flex items-center gap-1 ml-1"><Icon name="circle-check" size={12} /> เวอร์ชันล่าสุด v{shownVer}</span>
            {:else}<span class="ver-badge old inline-flex items-center gap-1 ml-1"><Icon name="alert-triangle" size={12} /> เวอร์ชันเก่า v{shownVer} · ล่าสุด v{latestVer}</span>{/if}
          {/if}
          {#if versions[0] && versions[0].source !== 'original'}<span class="mut"> · อัปเดตล่าสุด {fmtWhen(versions[0].signedAt)} โดย {versions[0].signerName}</span>{/if}
          {#if editing}<span class="ver-badge latest inline-flex items-center gap-1 ml-1"><Icon name="pencil" size={12} /> กำลังแก้ไข</span>{/if}
        </div>
      </div>
      <button class="mx inline-flex items-center justify-center" on:click={close}><Icon name="x" size={16} /></button>
    </header>

    <nav class="mtabs">
      <button class={tab === 'doc' ? 'active' : ''} on:click={() => (tab = 'doc')}>เอกสาร</button>
      <button class={tab === 'hist' ? 'active' : ''} on:click={() => (tab = 'hist')}>ประวัติเวอร์ชัน ({versions.length})</button>
      <span class="grow"></span>
      {#if !loading && tab === 'doc' && content}
        <a class="mbtn gh inline-flex items-center gap-1.5" href={dl(ver)} download><Icon name="download" size={14} /> ดาวน์โหลด{ver ? ` v${ver}` : ''}</a>
        {#if canEdit}<EditActions {editing} {dirty} on:cancel={cancelEdit} on:start={startEdit} on:update={beginUpdate} />{/if}
      {/if}
    </nav>
    <input bind:this={uploadInput} type="file" class="hidden" on:change={onPickFile} />

    <div class="mbody">
      {#if savedMsg}<div class="save-ok inline-flex items-center gap-1.5"><Icon name="circle-check" size={16} /> {savedMsg}</div>{/if}
      {#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={() => load(ver)}>ลองใหม่</button></div>{/if}
      {#if loading}
        <div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังเปิดเอกสาร…</p></div>
      {:else if tab === 'hist'}
        {#if !versions.length}
          <div class="empty-state"><Icon name="clock" size={34} stroke={1.5} /><p>ยังไม่มีการแก้ไขหรืออัปโหลด — ไฟล์นี้ยังเป็นต้นฉบับจากแฟ้ม</p></div>
        {:else}
          <ChangeHistory {versions} currentVer={shownVer} showSigner>
            <svelte:fragment slot="actions" let:v>
              {#if v.version !== shownVer}<button class="mbtn gh" on:click={() => openVersion(v.version)}>เปิดดู</button>{/if}
              <a class="mbtn gh" href={dl(v.version)} download>ดาวน์โหลด</a>
            </svelte:fragment>
          </ChangeHistory>
          <div class="vtree">
            {#each versions as v}
              <div class="vrow {v.version === shownVer ? 'cur' : ''}">
                <div class="vcard">
                  <div class="vtop"><span class="vn">Ver {v.version}</span><span class="vsrc {v.source}">{srcLabel(v.source)}</span>{#if v.version === latestVer}<span class="ver-badge latest">ล่าสุด</span>{/if}</div>
                  <div class="vwho inline-flex items-center gap-1.5"><Icon name="pen-line" size={14} /> <b>{v.signerName}</b>{#if v.signerRole} · {v.signerRole}{/if}</div>
                  <div class="vmeta inline-flex items-center gap-1.5"><Icon name="clock" size={12} /> {fmtWhen(v.signedAt)} · {v.filename}{#if v.size} · {(v.size / 1024).toFixed(0)} KB{/if}</div>
                  {#if v.note}<div class="vnote inline-flex items-center gap-1.5"><Icon name="note" size={13} /> {v.note}</div>{/if}
                </div>
              </div>
            {/each}
          </div>
        {/if}
      {:else if content}
        <div class="file-tools">
          <div class="ft-head">
            <b>การแก้ไขไฟล์</b>
            <span class="mut">แก้บนเว็บหรืออัปโหลดไฟล์ใหม่ — ทุกครั้งบันทึกเป็นเวอร์ชันใหม่ พร้อมผู้แก้ไขและวันเวลา ไม่ทับเวอร์ชันเดิม</span>
          </div>
          <div class="ft-actions">
            {#if canEdit}
              <EditActions {editing} {dirty} accent editLabel="แก้ไขไฟล์บนเว็บ" on:cancel={cancelEdit} on:start={startEdit} on:update={beginUpdate} />
            {:else if writer && isLatest}
              <span class="ft-hint">ไฟล์ชนิดนี้แก้บนเว็บไม่ได้ — ใช้อัปโหลดไฟล์เวอร์ชันใหม่</span>
            {/if}
            {#if writer && isLatest && !editing}
              <button class="mbtn inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="cloud-upload" size={14} /> อัปโหลดไฟล์เวอร์ชันใหม่</button>
            {/if}
            <a class="mbtn gh inline-flex items-center gap-1.5" href={dl(ver)} download><Icon name="download" size={14} /> ดาวน์โหลด</a>
            <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => (tab = 'hist')}><Icon name="clock" size={14} /> ประวัติเวอร์ชัน ({versions.length})</button>
          </div>
        </div>

        {#if !isLatest}
          <div class="ver-warn">
            <Icon name="alert-triangle" size={16} />
            <span>กำลังดูเวอร์ชันเก่า (v{shownVer}) — ฉบับล่าสุดคือ <b>v{latestVer}</b></span>
            <button class="vw-btn" on:click={() => load()}>ไปเวอร์ชันล่าสุด</button>
          </div>
        {/if}
        <ChangeHistory {versions} currentVer={shownVer} onDoc />

        <DocBody bind:this={body} {content} name={file.name} origUrl={view(ver)} embedUrl={view(ver)} downloadUrl={dl(ver)} {editing} bind:dirty />
      {/if}
    </div>

    {#if signing}
      <SignDialog
        title={file.name}
        summary={signing === 'edit'
          ? `แก้ไขไฟล์บนเว็บ → บันทึกเป็นเวอร์ชัน ${nextV()} (เวอร์ชันเดิมยังเก็บไว้)`
          : `อัปโหลด “${pendingUpload?.name}” → บันทึกเป็นเวอร์ชัน ${nextV()} (เวอร์ชันเดิมยังเก็บไว้)`}
        confirmLabel="บันทึกเป็นเวอร์ชันใหม่"
        fullChange={true}
        {me}
        on:confirm={doSave}
        on:cancel={() => { signing = null; pendingUpload = null }}
      />
    {/if}

    {#if confirmAction}
      <ConfirmDialog title="ยืนยันการทำรายการ" message={confirmAction.message} confirmLabel="ดำเนินการต่อ" danger
        on:confirm={() => { confirmAction.run(); confirmAction = null }} on:cancel={() => (confirmAction = null)} />
    {/if}
  </div>
</div>
