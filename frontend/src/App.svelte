<script>
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import Dashboard from './Dashboard.svelte'
  import Overview from './Overview.svelte'
  import Procedures from './Procedures.svelte'
  import Register from './Register.svelte'
  import Actions from './Actions.svelte'
  import AuditHub from './AuditHub.svelte'
  import Changes from './Changes.svelte'
  import RevControl from './RevControl.svelte'
  import Verify from './Verify.svelte'
  import ManagementReview from './ManagementReview.svelte'
  import InternalAudit from './InternalAudit.svelte'
  import Acks from './Acks.svelte'
  import TrainingMatrix from './TrainingMatrix.svelte'
  import AiChat from './AiChat.svelte'
  import Users from './Users.svelte'
  import Kpi from './Kpi.svelte'
  import Risks from './Risks.svelte'
  import Calibration from './Calibration.svelte'
  import Suppliers from './Suppliers.svelte'
  import Inbox from './Inbox.svelte'
  import AuditTrail from './AuditTrail.svelte'
  import Forms from './Forms.svelte'
  import Login from './Login.svelte'
  import ForcePin from './ForcePin.svelte'
  import Checkin from './Checkin.svelte'
  import DocViewer from './DocViewer.svelte'
  import FilePreview from './FilePreview.svelte'
  import Icon from './Icon.svelte'

  const TABS = ['dash', 'ov', 'proc', 'reg', 'dcr', 'rev', 'act', 'mr', 'audit', 'forms', 'users', 'trail', 'ia', 'ack', 'tm', 'kpi', 'risk', 'cal', 'sup']
  let tab = 'dash'
  let docs = [], overview = []
  let openCode = null
  let openChangeId = null // เปิด DocViewer ที่คำขอเปลี่ยนแปลงใบนี้
  let pendingDocs = 0
  let pendingAcks = 0
  let dcrRefresh = 0
  let ovRefresh = 0
  let ovFile = null
  let loaded = false
  let loadErr = ''
  let slim = false
  const JUMP_BASE = { type: '', dept: '', unreg: false, section: '', q: '' }
  let jump = { n: 0, ...JUMP_BASE }
  let me = null
  let aiEnabled = false // แชทผู้ช่วย ISO แสดงเมื่อเซิร์ฟเวอร์ตั้ง OPENAI_API_KEY แล้ว
  let pending = 0
  let checkinToken = ''
  let verifyToken = '' // #v=<token> จาก QR บนกระดาษ — หน้าตรวจสถานะเอกสาร ไม่ต้อง login
  let formLaunch = null
  let focus = { kpi: '', risk: '', cal: '', sup: '' } // เปิดรายการเฉพาะเมื่อมาจากงานของฉัน/เตรียม Audit
  let inboxRefresh = 0

  const NAV = [
    { group: 'ภาพรวม', items: [['dash', 'dashboard', 'แดชบอร์ด']] },
    { group: 'คลังเอกสาร', items: [
      ['ov', 'folder', '1 · ภาพรวมองค์กร'],
      ['proc', 'clipboard-list', '2 · วิธีปฏิบัติงาน และเอกสารแนบ'],
      ['reg', 'library', 'ทะเบียนเอกสารทั้งหมด'],
    ] },
    { group: 'ติดตามงาน', items: [
      ['dcr', 'clipboard-check', 'คำขอเปลี่ยนแปลงเอกสาร'],
      ['rev', 'shield-check', 'ควบคุมฉบับ / สำเนา'],
      ['ack', 'circle-check', 'การรับทราบเอกสาร'],
      ['tm', 'user', 'Training matrix'],
      ['forms', 'clipboard-list', 'บันทึกแบบฟอร์ม'],
      ['act', 'clipboard-check', 'CAR / DAR / PAR'],
      ['ia', 'list-checks', 'ตรวจติดตามภายใน'],
      ['risk', 'shield-alert', 'ความเสี่ยงและโอกาส'],
      ['kpi', 'target', 'วัตถุประสงค์คุณภาพ (KPI)'],
      ['cal', 'gauge', 'เครื่องมือวัดและสอบเทียบ'],
      ['sup', 'truck', 'ผู้ขายและการประเมิน'],
      ['mr', 'building', 'ทบทวนฝ่ายบริหาร'],
      ['audit', 'list-checks', 'เตรียม Audit'],
    ] },
  ]

  // เมนูผู้ดูแลระบบแสดงเฉพาะ admin (server ตรวจสิทธิ์ซ้ำทุกคำขอ)
  $: adminItems = [
    ...(['approver', 'admin'].includes(me?.role) ? [['trail', 'list-checks', 'บันทึกการใช้งาน']] : []),
    ...(me?.role === 'admin' ? [['users', 'user', 'ผู้ใช้และสิทธิ์']] : []),
  ]
  $: nav = adminItems.length ? [...NAV, { group: 'ผู้ดูแลระบบ / ตรวจสอบ', items: adminItems }] : NAV

  async function loadDocs() { docs = await api.docs() }
  async function loadAll() {
    loadErr = ''
    try {
      await Promise.all([loadDocs(), api.overview().then((o) => (overview = o))])
      api.aiStatus().then((a) => (aiEnabled = !!a?.enabled)).catch(() => {})
      loaded = true
    } catch (e) { loadErr = 'โหลดข้อมูลไม่สำเร็จ: ' + e.message }
  }
  function loggedIn(user) {
    me = user
    if (user?.mustChangePin) return
    loadAll()
    refreshPending()
  }
  function refreshPending() {
    inboxRefresh += 1
    api.me().then((m) => { pending = m.pending || 0; pendingDocs = m.pendingDocs || 0; pendingAcks = m.pendingAcks || 0 }).catch(() => {})
  }
  async function logout() {
    try { await api.logout() } catch (e) {}
    localStorage.removeItem('isoToken')
    me = null
  }
  function onEform(e) {
    formLaunch = { code: e.detail.code, mode: e.detail.mode, n: Date.now() }
    openCode = null
    goTab('forms')
  }
  onMount(async () => {
    const hash = location.hash || ''
    if (hash.startsWith('#checkin=')) checkinToken = decodeURIComponent(hash.slice('#checkin='.length))
    if (hash.startsWith('#v=')) verifyToken = decodeURIComponent(hash.slice('#v='.length))
    const onUnauth = () => { me = null }
    window.addEventListener('iso-unauth', onUnauth)
    try { const t = localStorage.getItem('activeTab'); if (t && TABS.includes(t)) tab = t } catch (e) {}
    try { slim = localStorage.getItem('sidebarSlim') === '1' } catch (e) {}
    if (!checkinToken && !verifyToken && localStorage.getItem('isoToken')) {
      try {
        const m = await api.me()
        me = m.user
        pending = m.pending || 0
        pendingDocs = m.pendingDocs || 0
        pendingAcks = m.pendingAcks || 0
        if (!me?.mustChangePin) await loadAll()
      } catch (e) { me = null }
    }
  })
  function goTab(k) {
    tab = k
    try { localStorage.setItem('activeTab', k) } catch (e) {}
  }
  function toggleSidebar() {
    slim = !slim
    try { localStorage.setItem('sidebarSlim', slim ? '1' : '0') } catch (e) {}
  }
  function setJump(patch) {
    jump = { n: jump.n + 1, ...JUMP_BASE, ...patch }
  }
  function onGoto(e) {
    const d = e.detail || {}
    setJump({ type: d.type || '', dept: d.dept || '', unreg: !!d.unreg, section: d.section || '' })
    if (d.ref && ['kpi', 'risk', 'cal', 'sup'].includes(d.tab)) focus = { ...focus, [d.tab]: d.ref }
    goTab(d.tab)
  }
  /** คลิกงานในกระดิ่ง: ไปหน้าที่เกี่ยวข้อง (KPI/ความเสี่ยงเปิดรายการนั้นทันที) — เปลี่ยน key เพื่อโหลดหน้าใหม่แม้อยู่หน้าเดิม */
  let tabKey = 0
  function onInbox(e) {
    onGoto(e)
    tabKey += 1
  }

  // ---- ค้นหาส่วนกลาง (ทะเบียน + ภาพรวมองค์กร) ----
  let gq = ''
  let gsFocus = -1
  $: ovFlat = (overview || []).flatMap((s) =>
    (s.groups || []).flatMap((g) =>
      (g.items || []).map((f) => ({ ...f, section: s.name, sectionKey: s.key }))))
  $: gsNeedle = gq.toLowerCase()
  $: gsDocs = gq.trim().length < 1 ? [] : docs
    .filter((d) => d.code.toLowerCase().includes(gsNeedle) || d.name.toLowerCase().includes(gsNeedle))
    .slice(0, 8)
  $: gsOv = gq.trim().length < 1 ? [] : ovFlat
    .filter((f) => f.name.toLowerCase().includes(gsNeedle))
    .slice(0, 8)
  $: gsAll = [...gsDocs.map((d) => ({ k: 'doc', d })), ...gsOv.map((f) => ({ k: 'ov', f }))]
  function clearSearch() { gq = ''; gsFocus = -1 }
  function gsOpenDoc(code) {
    openCode = code; ovFile = null; clearSearch()
  }
  /** เปิดไฟล์ภาพรวม — ถ้าอยู่ในทะเบียนให้ใช้ DocViewer (แก้แล้วอัปเดทเป็นเวอร์ชันใหม่) */
  function openOvFile(f) {
    const norm = (p) => (p || '').replace(/\\/g, '/')
    const registered = docs.find((d) => norm(d.path) === norm(f.path))
    openCode = registered?.code ?? null
    openChangeId = null
    ovFile = registered ? null : f
  }
  function gsOpenOv(f) {
    setJump({ section: f.sectionKey })
    goTab('ov')
    openOvFile(f)
    clearSearch()
  }
  function gsPick(it) {
    if (it.k === 'doc') gsOpenDoc(it.d.code)
    else gsOpenOv(it.f)
  }
  function gsKey(e) {
    if (!gsAll.length) return
    if (e.key === 'ArrowDown') { e.preventDefault(); gsFocus = Math.min(gsFocus + 1, gsAll.length - 1) }
    else if (e.key === 'ArrowUp') { e.preventDefault(); gsFocus = Math.max(gsFocus - 1, 0) }
    else if (e.key === 'Enter' && gsFocus >= 0) gsPick(gsAll[gsFocus])
    else if (e.key === 'Escape') clearSearch()
  }
  function gsBlur() {
    setTimeout(() => { gq = '' }, 150)
  }

  $: openDoc = openCode ? docs.find((d) => d.code === openCode) : null
  function open(e) { openCode = e.detail; openChangeId = null; ovFile = null }
  function openChange(e) { openCode = e.detail.code; openChangeId = e.detail.change; ovFile = null }
  function closeDoc() { openCode = null; openChangeId = null }
  async function onSaved() {
    await loadDocs()
    refreshPending()
    dcrRefresh += 1
    ovRefresh += 1
  }
</script>

{#if checkinToken}
  <Checkin token={checkinToken} />
{:else if verifyToken}
  <Verify token={verifyToken} />
{:else if !me}
  <Login onOk={loggedIn} />
{:else if me.mustChangePin}
  <ForcePin {me} onDone={() => { localStorage.removeItem('isoToken'); me = null }} onLogout={logout} />
{:else}
<div class="app">
  <aside class="sidebar {slim ? 'slim' : ''}">
    <div class="brand">
      <div class="logo">KG</div>
      <div class="tx"><b>ระบบควบคุมเอกสาร</b><small>ISO 9001:2015 · v2</small></div>
    </div>
    <nav>
      {#each nav as g}
        <div class="navgroup-label">{g.group}</div>
        {#each g.items as [k, ic, label]}
          <button class={tab === k ? 'active' : ''} title={label} on:click={() => { focus = { kpi: '', risk: '', cal: '', sup: '' }; goTab(k) }}>
            <span class="ic"><Icon name={ic} size={18} /></span><span class="lb">{label}{#if k === 'forms' && pending} ({pending}){/if}{#if k === 'dcr' && pendingDocs} ({pendingDocs}){/if}{#if k === 'ack' && pendingAcks} ({pendingAcks}){/if}</span>
          </button>
        {/each}
      {/each}
    </nav>
    <div class="sidebar-foot"><button class="collapse-btn inline-flex items-center justify-center gap-1.5" on:click={toggleSidebar}>{#if slim}<Icon name="chevrons-right" size={16} />{:else}<Icon name="chevrons-left" size={16} /> ย่อเมนู{/if}</button></div>
  </aside>

  <div class="main">
    <header class="topbar">
      <div class="tb-row">
        <div>
          <h1>ระบบควบคุมเอกสาร ISO 9001:2015</h1>
          <p>บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด · Update 2-11-2568 · ค้นหาแล้วคลิกเปิดดูได้ทุกฉบับ</p>
        </div>
        {#if loaded}
          <div class="gsearch">
            <span class="gs-ic"><Icon name="search" size={16} /></span>
            <input
              placeholder="ค้นหาทุกเอกสาร — รหัส, ชื่อ, หรือไฟล์ภาพรวมองค์กร..."
              bind:value={gq}
              on:keydown={gsKey}
              on:blur={gsBlur}
            />
            {#if gq.trim()}
              <div class="gs-results">
                {#if gsAll.length}
                  {#if gsDocs.length}
                    <div class="gs-sec">ทะเบียนเอกสาร</div>
                    {#each gsDocs as d, i}
                      <div
                        class="gs-item {gsFocus === i ? 'sel' : ''}"
                        role="button" tabindex="-1"
                        on:mousedown|preventDefault={() => gsOpenDoc(d.code)}
                      >
                        <span class="code">{d.code}</span>
                        <span class="nm">{d.name}</span>
                        <span class="dp">{d.deptName}</span>
                      </div>
                    {/each}
                  {/if}
                  {#if gsOv.length}
                    <div class="gs-sec">ภาพรวมองค์กร</div>
                    {#each gsOv as f, i}
                      <div
                        class="gs-item {gsFocus === gsDocs.length + i ? 'sel' : ''}"
                        role="button" tabindex="-1"
                        on:mousedown|preventDefault={() => gsOpenOv(f)}
                      >
                        <span class="ov-ic text-brand"><Icon name="folder" size={14} /></span>
                        <span class="nm">{f.name}</span>
                        <span class="dp">{f.section}</span>
                      </div>
                    {/each}
                  {/if}
                {:else}
                  <div class="gs-empty">ไม่พบเอกสารที่ตรงกับ “{gq}”</div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
        {#if me}
          <div class="flex items-center gap-2">
            <Inbox refresh={inboxRefresh} on:go={onInbox} />
            <div class="note m-0 whitespace-nowrap">{me.name}<button class="mbtn gh" on:click={logout}>ออก</button></div>
          </div>
        {/if}
      </div>
    </header>

    <div class="wrap">
      {#if loadErr}
        <div class="merr"><span>{loadErr}</span><button class="retry" on:click={loadAll}>ลองใหม่</button></div>
      {/if}
      {#if !loaded}
        <div class="loadwrap"><span class="spin"></span><p class="note m-0">กำลังโหลด…</p></div>
      {:else if tab === 'dash'}
        <Dashboard {docs} {overview} on:open={open} on:goto={onGoto} />
      {:else if tab === 'ov'}
        <Overview {overview} {jump} refresh={ovRefresh} on:preview={(e) => openOvFile(e.detail)} />
      {:else if tab === 'proc'}
        <Procedures {docs} {jump} on:open={open} />
      {:else if tab === 'reg'}
        <Register {docs} {jump} on:open={open} />
      {:else if tab === 'dcr'}
        <Changes {me} refresh={dcrRefresh} on:open={openChange} />
      {:else if tab === 'rev'}
        {#key tabKey}<RevControl {me} refresh={dcrRefresh} on:open={open} />{/key}
      {:else if tab === 'tm'}
        <TrainingMatrix {me} {docs} on:open={open} />
      {:else if tab === 'ack'}
        <Acks {me} {docs} refresh={dcrRefresh} on:open={open} />
      {:else if tab === 'ia'}
        <InternalAudit {me} {docs} />
      {:else if tab === 'mr'}
        <ManagementReview {me} on:doc={(e) => { openCode = docs.some((d) => d.code === e.detail) ? e.detail : openCode }} />
      {:else if tab === 'trail'}
        <AuditTrail {me} />
      {:else if tab === 'users'}
        <Users {me} />
      {:else if tab === 'kpi'}
        {#key tabKey}<Kpi {me} focus={focus.kpi} on:goto={onGoto} />{/key}
      {:else if tab === 'risk'}
        {#key tabKey}<Risks {me} focus={focus.risk} on:goto={onGoto} />{/key}
      {:else if tab === 'cal'}
        {#key tabKey}<Calibration {me} focus={focus.cal} on:goto={onGoto} />{/key}
      {:else if tab === 'sup'}
        {#key tabKey}<Suppliers {me} focus={focus.sup} on:goto={onGoto} />{/key}
      {:else if tab === 'act'}
        <Actions {docs} {me} />
      {:else if tab === 'audit'}
        <AuditHub {docs} {me} on:open={open} on:goto={onGoto} />
      {:else if tab === 'forms'}
        <Forms launch={formLaunch} {me} />
      {/if}
    </div>

    <footer>ระบบควบคุมเอกสาร ISO 9001:2015 · v2 · หน้าเว็บ Svelte+Bun · API Rust · ONLYOFFICE</footer>
  </div>
</div>

{#if me && aiEnabled}<AiChat {docs} on:open={open} />{/if}
{#if openDoc}
  {#key openDoc.code}
    <DocViewer doc={openDoc} {me} initialChange={openChangeId} on:close={closeDoc} on:saved={onSaved} on:eform={onEform} />
  {/key}
{/if}
{#if ovFile}
  {#key ovFile.path}
    <FilePreview file={ovFile} {me} on:close={() => (ovFile = null)} on:saved={onSaved} />
  {/key}
{/if}
{/if}
