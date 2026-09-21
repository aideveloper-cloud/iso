<script>
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import Dashboard from './Dashboard.svelte'
  import Overview from './Overview.svelte'
  import Procedures from './Procedures.svelte'
  import Register from './Register.svelte'
  import DocViewer from './DocViewer.svelte'

  let tab = 'dash'
  let docs = [], overview = []
  let openCode = null
  let loaded = false
  let slim = false

  const NAV = [
    { group: 'ภาพรวม', items: [['dash', '📊', 'แดชบอร์ด']] },
    { group: 'คลังเอกสาร', items: [
      ['ov', '📁', '1 · ภาพรวมองค์กร'],
      ['proc', '📋', '2 · วิธีปฏิบัติงานแต่ละแผนก'],
      ['reg', '🗂️', 'ทะเบียนเอกสารทั้งหมด'],
    ] },
  ]

  async function loadDocs() { docs = await api.docs() }
  onMount(async () => {
    await Promise.all([loadDocs(), api.overview().then((o) => (overview = o))])
    loaded = true
    try { slim = localStorage.getItem('sidebarSlim') === '1' } catch (e) {}
  })
  function toggleSidebar() {
    slim = !slim
    try { localStorage.setItem('sidebarSlim', slim ? '1' : '0') } catch (e) {}
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
          <button class={tab === k ? 'active' : ''} title={label} on:click={() => (tab = k)}>
            <span class="ic">{ic}</span><span class="lb">{label}</span>
          </button>
        {/each}
      {/each}
    </nav>
    <div class="sidebar-foot"><button class="collapse-btn" on:click={toggleSidebar}>{slim ? '»' : '« ย่อเมนู'}</button></div>
  </aside>

  <div class="main">
    <header class="topbar">
      <h1>ระบบควบคุมเอกสาร ISO 9001:2015</h1>
      <p>บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด (K Garden &amp; Fence) · รุ่นล่าสุด Update 2-11-2568 · <b>v2 (Svelte + Rust + Go)</b></p>
    </header>

    <div class="wrap">
      {#if !loaded}
        <p class="note">กำลังโหลด…</p>
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
