<script>
  // เตรียม Audit: ความพร้อม (คัดกรองจากข้อมูลจริง) · ตรวจตามข้อกำหนด · ดัชนีเอกสารสำหรับพิมพ์
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import Readiness from './Readiness.svelte'
  import Clauses from './Clauses.svelte'
  import Audit from './Audit.svelte'
  import Icon from './Icon.svelte'

  export let docs = []
  export let me = null
  const dispatch = createEventDispatcher()

  const VIEWS = [['ready', 'ความพร้อมรับการตรวจ'], ['clause', 'ตรวจตามข้อกำหนด 4–10'], ['index', 'ดัชนีเอกสาร (พิมพ์ให้ Auditor)']]
  let view = 'ready'
  let r = null, err = '', busy = false

  try { const v = localStorage.getItem('auditView'); if (VIEWS.some(([k]) => k === v)) view = v } catch (e) {}
  function setView(k) {
    view = k
    try { localStorage.setItem('auditView', k) } catch (e) {}
  }

  async function load() {
    busy = true
    err = ''
    try { r = await api.readiness() } catch (e) { err = 'คำนวณความพร้อมไม่สำเร็จ: ' + e.message }
    busy = false
  }
  onMount(load)
  // เอกสารเปลี่ยน (เช่น อนุมัติ DCR) → คำนวณใหม่
  let seen = docs
  $: if (docs !== seen) { seen = docs; if (r) load() }
</script>

<div class="cat-head no-print">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="list-checks" size={20} /> เตรียม Audit</h2>
  <p class="cat-desc">คัดกรองความพร้อมจากข้อมูลจริงในระบบ — เอกสาร DCR CAR/DAR/PAR บันทึกแบบฟอร์ม และหลักฐานตามข้อกำหนด ISO 9001:2015</p>
</div>

<div class="subtabs no-print">
  {#each VIEWS as [k, label]}
    <button class="st-btn {view === k ? 'active' : ''}" on:click={() => setView(k)}>{label}</button>
  {/each}
</div>

{#if view === 'index'}
  <Audit {docs} on:open />
{:else if err}
  <div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>
{:else if !r}
  <div class="loadwrap"><span class="spin"></span></div>
{:else if view === 'ready'}
  <Readiness {r} {me} {busy} on:reload={load} on:open on:goto />
{:else}
  <Clauses {r} on:open />
{/if}
