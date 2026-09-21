<script>
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import Dashboard from './Dashboard.svelte'
  import Overview from './Overview.svelte'
  import Procedures from './Procedures.svelte'
  import Register from './Register.svelte'
  import DocViewer from './DocViewer.svelte'
  import Icon from './Icon.svelte'

  const TABS = ['dash', 'ov', 'proc', 'reg']
  let tab = 'dash'
  let docs = [], overview = []
  let openCode = null
  let loaded = false
  let loadErr = ''
  let slim = false

  const NAV = [
    { group: 'ภาพรวม', items: [['dash', 'dashboard', 'แดชบอร์ด']] },
    { group: 'คลังเอกสาร', items: [
      ['ov', 'folder', '1 · ภาพรวมองค์กร'],
      ['proc', 'clipboard-list', '2 · วิธีปฏิบัติงานแต่ละแผนก'],
      ['reg', 'library', 'ทะเบียนเอกสารทั้งหมด'],
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

  // ---- ค้นหาส่วนกลาง ----
  let gq = ''
  let gsFocus = -1
  $: gsResults = gq.trim().length < 1 ? [] : docs
    .filter((d) => d.code.toLowerCase().includes(gq.toLowerCase()) || d.name.toLowerCase().includes(gq.toLowerCase()))
    .slice(0, 12)
  function gsOpen(code) {
    openCode = code; gq = ''; gsFocus = -1
  }
  function gsKey(e) {
    if (!gsResults.length) return
    if (e.key === 'ArrowDown') { e.preventDefault(); gsFocus = Math.min(gsFocus + 1, gsResults.length - 1) }
    else if (e.key === 'ArrowUp') { e.preventDefault(); gsFocus = Math.max(gsFocus - 1, 0) }
    else if (e.key === 'Enter' && gsFocus >= 0) { gsOpen(gsResults[gsFocus].code) }
    else if (e.key === 'Escape') { gq = ''; gsFocus = -1 }
  }
  function gsBlur() {
    // หน่วงเล็กน้อยให้คลิกในผลลัพธ์ทำงานก่อน blur ปิดรายการ
    setTimeout(() => { gq = '' }, 150)
  }

  $: openDoc = openCode ? docs.find((d) => d.code === openCode) : null
  const open = (e) => (openCode = e.detail)
  async function onSaved() { await loadDocs() } // รีเฟรชป้ายเวอร์ชัน
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
      <h1>ระบบควบคุมเอกสาร ISO 9001:2015</h1>
      <p>บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด (K Garden &amp; Fence) · รุ่นล่าสุด Update 2-11-2568 · <b>v2 (Svelte + Rust + Go)</b></p>
      {#if loaded}
        <div class="gsearch">
          <span class="gs-ic"><Icon name="search" size={16} /></span>
          <input
            placeholder="ค้นหาเอกสารทั้งหมดด้วยรหัสหรือชื่อ..."
            bind:value={gq}
            on:keydown={gsKey}
            on:blur={gsBlur}
          />
          {#if gq.trim()}
            <div class="gs-results">
              {#if gsResults.length}
                {#each gsResults as d, i}
                  <div
                    class="gs-item {i === gsFocus ? 'sel' : ''}"
                    role="button" tabindex="-1"
                    on:mousedown|preventDefault={() => gsOpen(d.code)}
                  >
                    <span class="code">{d.code}</span>
                    <span class="nm">{d.name}</span>
                    <span class="dp">{d.deptName}</span>
                  </div>
                {/each}
              {:else}
                <div class="gs-empty">ไม่พบเอกสารที่ตรงกับ “{gq}”</div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </header>

    <div class="wrap">
      {#if loadErr}
        <div class="merr"><span>{loadErr}</span><button class="retry" on:click={loadAll}>ลองใหม่</button></div>
      {/if}
      {#if !loaded}
        <div class="loadwrap"><span class="spin"></span><p class="note m-0">กำลังโหลด…</p></div>
      {:else if tab === 'dash'}
        <Dashboard {docs} />
      {:else if tab === 'ov'}
        <Overview {overview} />
      {:else if tab === 'proc'}
        <Procedures {docs} on:open={open} />
      {:else if tab === 'reg'}
        <Register {docs} on:open={open} />
      {/if}
    </div>

    <footer>ระบบควบคุมเอกสาร ISO 9001:2015 · v2 · หน้าเว็บ Svelte+Bun · API Rust · แปลงเอกสาร Go</footer>
  </div>
</div>

{#if openDoc}
  <DocViewer doc={openDoc} on:close={() => (openCode = null)} on:saved={onSaved} />
{/if}
