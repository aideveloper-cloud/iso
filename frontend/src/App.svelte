<script>
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import Dashboard from './Dashboard.svelte'
  import Overview from './Overview.svelte'
  import Procedures from './Procedures.svelte'
  import Register from './Register.svelte'
  import Actions from './Actions.svelte'
  import Audit from './Audit.svelte'
  import DocViewer from './DocViewer.svelte'
  import FilePreview from './FilePreview.svelte'
  import Icon from './Icon.svelte'

  const TABS = ['dash', 'ov', 'proc', 'reg', 'act', 'audit']
  let tab = 'dash'
  let docs = [], overview = []
  let openCode = null
  let ovFile = null
  let loaded = false
  let loadErr = ''
  let slim = false
  const JUMP_BASE = { type: '', dept: '', unreg: false, section: '', q: '' }
  let jump = { n: 0, ...JUMP_BASE }

  const NAV = [
    { group: 'ภาพรวม', items: [['dash', 'dashboard', 'แดชบอร์ด']] },
    { group: 'คลังเอกสาร', items: [
      ['ov', 'folder', '1 · ภาพรวมองค์กร'],
      ['proc', 'clipboard-list', '2 · วิธีปฏิบัติงาน และเอกสารแนบ'],
      ['reg', 'library', 'ทะเบียนเอกสารทั้งหมด'],
    ] },
    { group: 'ติดตามงาน', items: [
      ['act', 'clipboard-check', 'CAR / DAR / PAR'],
      ['audit', 'list-checks', 'เตรียม Audit'],
    ] },
  ]

  async function loadDocs() { docs = await api.docs() }
  async function loadAll() {
    loadErr = ''
    try {
      await Promise.all([loadDocs(), api.overview().then((o) => (overview = o))])
      loaded = true
    } catch (e) { loadErr = 'โหลดข้อมูลไม่สำเร็จ: ' + e.message }
  }
  onMount(async () => {
    try { const t = localStorage.getItem('activeTab'); if (t && TABS.includes(t)) tab = t } catch (e) {}
    try { slim = localStorage.getItem('sidebarSlim') === '1' } catch (e) {}
    await loadAll()
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
    goTab(d.tab)
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
  function open(e) { openCode = e.detail; ovFile = null }
  async function onSaved() { await loadDocs() }
</script>

<div class="app">
  <aside class="sidebar {slim ? 'slim' : ''}">
    <div class="brand">
      <div class="logo">KG</div>
      <div class="tx"><b>ระบบควบคุมเอกสาร</b><small>ISO 9001:2015 · v2</small></div>
    </div>
    <nav>
      {#each NAV as g}
        <div class="navgroup-label">{g.group}</div>
        {#each g.items as [k, ic, label]}
          <button class={tab === k ? 'active' : ''} title={label} on:click={() => goTab(k)}>
            <span class="ic"><Icon name={ic} size={18} /></span><span class="lb">{label}</span>
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
        <Overview {overview} {jump} on:preview={(e) => openOvFile(e.detail)} />
      {:else if tab === 'proc'}
        <Procedures {docs} {jump} on:open={open} />
      {:else if tab === 'reg'}
        <Register {docs} {jump} on:open={open} />
      {:else if tab === 'act'}
        <Actions {docs} />
      {:else if tab === 'audit'}
        <Audit {docs} on:open={open} />
      {/if}
    </div>

    <footer>ระบบควบคุมเอกสาร ISO 9001:2015 · v2 · หน้าเว็บ Svelte+Bun · API Rust · แปลงเอกสาร Go</footer>
  </div>
</div>

{#if openDoc}
  {#key openDoc.code}
    <DocViewer doc={openDoc} on:close={() => (openCode = null)} on:saved={onSaved} />
  {/key}
{/if}
{#if ovFile}
  {#key ovFile.path}
    <FilePreview file={ovFile} on:close={() => (ovFile = null)} on:saved={onSaved} />
  {/key}
{/if}
