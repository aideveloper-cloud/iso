<script>
  // แท็บ "สำเนา" ในหน้าเอกสาร — สำเนาควบคุมที่พิมพ์ + ไฟล์ต้นฉบับที่ดาวน์โหลด ของเอกสารนี้
  // ผู้ควบคุมเอกสาร (ผู้ตรวจสอบขึ้นไป) เห็นทุกสำเนาและออกสำเนาใหม่ได้ · คนอื่นเห็นเฉพาะที่ตนถือ
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import CopyTable from './CopyTable.svelte'
  import IssueCopyDialog from './IssueCopyDialog.svelte'
  import Icon from './Icon.svelte'

  export let doc
  export let me = null
  export let rev = ''
  /** ออกสำเนาได้เฉพาะฉบับปัจจุบันที่ยังใช้งาน */
  export let canIssue = false

  const reviewer = () => ['reviewer', 'approver', 'admin'].includes(me?.role)
  let rows = [], err = '', loading = true, issuing = false
  $: recall = rows.filter((c) => c.status === 'recall')
  $: active = rows.filter((c) => c.status === 'active')

  async function load() {
    loading = true; err = ''
    try { rows = await api.copies({ doc: doc.code }) } catch (e) { err = 'โหลดไม่สำเร็จ: ' + e.message }
    loading = false
  }
  onMount(load)
</script>

<div class="flex flex-wrap items-center gap-2 mb-3">
  <span class="mnote !m-0">
    {#if reviewer()}สำเนาที่ใช้งาน {active.length} · ต้องเรียกคืน {recall.length}{:else}สำเนาของเอกสารนี้ที่คุณถืออยู่{/if}
    — ทุกสำเนามี QR ให้สแกนตรวจว่ายังเป็นฉบับปัจจุบัน
  </span>
  <span class="flex-1"></span>
  {#if reviewer() && canIssue}
    <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => (issuing = true)}><Icon name="printer" size={14} /> ออกสำเนาควบคุม</button>
  {/if}
</div>
{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}
{#if loading}
  <div class="loadwrap"><span class="spin"></span></div>
{:else if !rows.length}
  <div class="empty-state"><Icon name="qr-code" size={34} stroke={1.5} /><p>ยังไม่มีสำเนาที่บันทึกไว้{#if reviewer() && canIssue} — กด “ออกสำเนาควบคุม” เมื่อต้องพิมพ์ไปใช้หน้างาน{/if}</p></div>
{:else}
  <CopyTable {rows} {me} showDoc={false} on:changed={load} />
{/if}

{#if issuing}
  <IssueCopyDialog {doc} {rev} on:issued={load} on:close={() => (issuing = false)} />
{/if}
