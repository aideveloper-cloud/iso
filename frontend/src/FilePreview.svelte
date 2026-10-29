<script>
  // ไฟล์ในคลังภาพรวมองค์กร (ไม่อยู่ในทะเบียน) — ดู · แก้ในตัวแก้ไขเอกสาร · อัปโหลดไฟล์ใหม่ ทุกครั้งเป็นเวอร์ชันใหม่ ไม่ทับของเดิม
  //   เก็บผู้แก้ไข วันเวลา ชื่อไฟล์ และย้อนดู/ดาวน์โหลดเวอร์ชันเก่าได้
  import { createEventDispatcher, onMount, onDestroy } from 'svelte'
  import { api, ovFileUrl, ovPdfUrl } from './lib/api.js'
  import { fileExt, fileKind, OFFICE_EXTS } from './lib/const.js'
  import { srcLabel, fmtWhen } from './lib/changeNote.js'
  import DocBody from './DocBody.svelte'
  import SignDialog from './SignDialog.svelte'
  import ChangeHistory from './ChangeHistory.svelte'
  import OfficeEdit from './OfficeEdit.svelte'
  import Icon from './Icon.svelte'

  export let file // { name, path, size? }
  export let me = null
  const dispatch = createEventDispatcher()

  let content = null
  let versions = []
  let ver = null // เวอร์ชันที่กำลังดู (null = ล่าสุด)
  let err = '', loading = true
  let signing = null // 'upload' | 'office' | null
  let gEdit = false // ตัวแก้ไขบนเว็บ (ONLYOFFICE)
  let gEditId = null
  let pendingUpload = null
  let uploadInput
  let savedMsg = ''
  let tab = 'doc'
  let manage = false // false = โหมดอ่าน (ค่าเริ่มต้น) · true = แสดงเครื่องมือแก้ไข/อัปโหลด

  $: latestVer = versions[0]?.version
  $: isLatest = !ver || ver === latestVer
  $: writer = me && me.role !== 'viewer'
  $: canWebOffice = writer && isLatest && OFFICE_EXTS.includes(fileExt(file))
  $: shownVer = ver || content?.version || null
  $: dl = (v) => ovFileUrl(file.path, true) + (v ? `&v=${v}` : '')
  $: view = (v) => ovFileUrl(file.path) + (v ? `&v=${v}` : '')
  const nextV = () => (latestVer || content?.version || 1) + 1

  onMount(() => { load().then(resumeWebOffice); window.addEventListener('keydown', onKey) })
  /** มีรอบแก้ไขค้างอยู่ → เปิดตัวแก้ไขต่อให้เลย */
  async function resumeWebOffice() {
    if (!canWebOffice) return
    try { if (await api.ovEditMine(file.path)) openWebOffice() } catch (e) { /* ไม่มีรอบค้าง */ }
  }
  function openWebOffice() { tab = 'doc'; manage = true; gEdit = true }
  onDestroy(() => window.removeEventListener('keydown', onKey))
  function onKey(e) { if (e.key === 'Escape' && !signing && !gEdit) close() }

  async function load(v = null) {
    loading = true; err = ''; content = null
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
      if (signing === 'office') {
        const { ok, body } = await api.editCommit(gEditId, note)
        if (!ok) { fail(body.error || 'บันทึกไม่สำเร็จ'); return }
        signing = null; gEdit = false; gEditId = null
        savedMsg = `บันทึกการแก้ไขสำเร็จ — เป็นเวอร์ชัน ${body.latestVersion} (เวอร์ชันก่อนหน้ายังเก็บไว้ครบ)`
        await load()
        dispatch('saved', body)
        setTimeout(() => { savedMsg = '' }, 8000)
        return
      }
      const { ok, body } = await api.overviewSave({ path: file.path, dataBase64: pendingUpload.b64, filename: pendingUpload.name, note })
      if (!ok) { fail(body.error || 'บันทึกไม่สำเร็จ'); return }
      const how = `อัปโหลด “${pendingUpload.name}”`
      signing = null; pendingUpload = null
      savedMsg = `${how}สำเร็จ — เป็นเวอร์ชัน ${body.latestVersion} (เวอร์ชันก่อนหน้ายังเก็บไว้ครบ)`
      await load()
      dispatch('saved', body)
      setTimeout(() => { savedMsg = '' }, 8000)
    } catch (ex) { fail('บันทึกไม่สำเร็จ: ' + ex.message) }
  }

  function close() { dispatch('close') }
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
            {#if isLatest}<span class="ver-badge latest inline-flex items-center gap-1 ml-1"><Icon name="circle-check" size={12} /> เวอร์ชันล่าสุด #{shownVer}</span>
            {:else}<span class="ver-badge old inline-flex items-center gap-1 ml-1"><Icon name="alert-triangle" size={12} /> เวอร์ชันเก่า #{shownVer} · ล่าสุด #{latestVer}</span>{/if}
          {/if}
          {#if versions[0] && versions[0].source !== 'original'}<span class="mut"> · อัปเดตล่าสุด {fmtWhen(versions[0].signedAt)} โดย {versions[0].signerName}</span>{/if}
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
        {#if manage}<button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => { manage = false; gEdit = false }}><Icon name="eye" size={14} /> กลับโหมดอ่าน</button>
        {:else if writer && isLatest}
          {#if canWebOffice}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={openWebOffice}><Icon name="pencil" size={14} /> แก้ไขเอกสาร</button>{/if}
          <button class="mbtn {canWebOffice ? 'gh' : 'accent'} inline-flex items-center gap-1.5" on:click={() => (manage = true)}><Icon name={canWebOffice ? 'cloud-upload' : 'pencil'} size={14} /> {canWebOffice ? 'อัปโหลดไฟล์ใหม่' : 'แก้ไข / อัปโหลด'}</button>
        {/if}
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
        {#if !manage}
          <div class="read-bar">
            <span class="read-mode inline-flex items-center gap-1.5"><Icon name="eye" size={14} /> โหมดอ่าน</span>
            <span class="mut">อ่านอย่างเดียว{#if writer && isLatest} — ต้องการแก้ไขให้กด “แก้ไข / อัปโหลด”{/if}</span>
            <span class="grow"></span>
            <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => (tab = 'hist')}><Icon name="clock" size={14} /> ประวัติ ({versions.length})</button>
          </div>
        {:else if !gEdit}
        <div class="file-tools">
          <div class="ft-head">
            <b>การแก้ไขไฟล์</b>
            <span class="mut">แก้ในตัวแก้ไขเอกสารหรืออัปโหลดไฟล์ใหม่ — ทุกครั้งบันทึกเป็นเวอร์ชันใหม่ พร้อมผู้แก้ไขและวันเวลา ไม่ทับเวอร์ชันเดิม</span>
          </div>
          <div class="ft-actions">
            {#if canWebOffice}
              <button class="mbtn accent inline-flex items-center gap-1.5" on:click={openWebOffice}><Icon name="pencil" size={14} /> แก้ไขเอกสาร</button>
            {:else if writer && isLatest}
              <span class="ft-hint">ไฟล์ชนิดนี้แก้บนเว็บไม่ได้ — ใช้อัปโหลดไฟล์เวอร์ชันใหม่</span>
            {/if}
            {#if writer && isLatest}
              <button class="mbtn inline-flex items-center gap-1.5" on:click={() => uploadInput.click()}><Icon name="cloud-upload" size={14} /> อัปโหลดไฟล์เวอร์ชันใหม่</button>
            {/if}
            <a class="mbtn gh inline-flex items-center gap-1.5" href={dl(ver)} download><Icon name="download" size={14} /> ดาวน์โหลด</a>
            <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => (tab = 'hist')}><Icon name="clock" size={14} /> ประวัติเวอร์ชัน ({versions.length})</button>
          </div>
        </div>
        <ChangeHistory {versions} currentVer={shownVer} onDoc />
        {/if}

        {#if !isLatest}
          <div class="ver-warn">
            <Icon name="alert-triangle" size={16} />
            <span>กำลังดูเวอร์ชันเก่า (#{shownVer}) — ฉบับล่าสุดคือ <b>#{latestVer}</b></span>
            <button class="vw-btn" on:click={() => load()}>ไปเวอร์ชันล่าสุด</button>
          </div>
        {/if}

        {#if gEdit && canWebOffice}
          <OfficeEdit ovPath={file.path} submitLabel="บันทึกเป็นเวอร์ชันใหม่"
            on:submit={(e) => { gEditId = e.detail.editId; signing = 'office' }}
            on:closed={() => (gEdit = false)} />
        {:else}
          <DocBody {content} name={file.name} embedUrl={view(ver)} downloadUrl={dl(ver)} pdfUrl={ovPdfUrl(file.path, ver)} />
        {/if}
      {/if}
    </div>

    {#if signing}
      <SignDialog
        title={file.name}
        summary={signing === 'office'
          ? `แก้ไขไฟล์บนเว็บ → บันทึกเป็นเวอร์ชัน ${nextV()} (เวอร์ชันเดิมยังเก็บไว้)`
          : `อัปโหลด “${pendingUpload?.name}” → บันทึกเป็นเวอร์ชัน ${nextV()} (เวอร์ชันเดิมยังเก็บไว้)`}
        confirmLabel="บันทึกเป็นเวอร์ชันใหม่"
        fullChange={true}
        {me}
        on:confirm={doSave}
        on:cancel={() => { signing = null; pendingUpload = null }}
      />
    {/if}

  </div>
</div>
