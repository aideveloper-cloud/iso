<script>
  // หน้าอ่านเอกสาร — ใช้ร่วมกันระหว่าง DocViewer (ทะเบียน), ChangePanel (ฉบับร่าง DCR) และ FilePreview (คลังภาพรวม)
  //   Word/Excel/PowerPoint/PDF : แสดง PDF ที่ API แปลง (Word ใช้ ONLYOFFICE ตัวเดียวกับตัวแก้ไข) พร้อมตราประทับ
  //   รูปภาพ : แสดงตรง · ไฟล์อื่น : ดาวน์โหลด
  //   การแก้ไขทำในตัวแก้ไขบนเว็บ (OfficeEdit.svelte / ONLYOFFICE) เท่านั้น
  import { onDestroy } from 'svelte'
  import Icon from './Icon.svelte'

  export let content // ผลจาก /content: { kind: 'pdf'|'embed'|'download', ext, reason? }
  export let name = ''
  export let embedUrl = ''
  export let downloadUrl = ''
  /** URL ไฟล์ PDF ที่แปลงจากต้นฉบับ */
  export let pdfUrl = ''

  let pdfSrc = ''
  let pdfState = 'idle' // idle | loading | ok | fail
  let pdfErr = ''
  let pdfFor = ''
  $: wantPdf = content?.kind === 'pdf' && !!pdfUrl
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

  // ดูเต็มจอ — มือถือบางรุ่น (iOS) ไม่รองรับ จึงมีปุ่มเปิดในแท็บใหม่ด้วย
  let pdfWrap
  let full = false
  const canFull = typeof document !== 'undefined' && !!document.documentElement.requestFullscreen
  function toggleFull() {
    if (document.fullscreenElement) document.exitFullscreen()
    else pdfWrap?.requestFullscreen().catch(() => {})
  }
</script>

<svelte:document on:fullscreenchange={() => (full = !!document.fullscreenElement && document.fullscreenElement === pdfWrap)} />

{#if wantPdf && pdfState === 'ok'}
  <div class="pdfwrap" bind:this={pdfWrap}>
    <div class="pdfbar">
      {#if canFull}<button class="mbtn gh inline-flex items-center gap-1.5" on:click={toggleFull}><Icon name={full ? 'x' : 'maximize'} size={14} /> {full ? 'ออกจากเต็มจอ' : 'เต็มจอ'}</button>{/if}
      <a class="mbtn gh inline-flex items-center gap-1.5" href={pdfSrc} target="_blank" rel="noopener"><Icon name="eye" size={14} /> เปิดในแท็บใหม่</a>
    </div>
    <iframe class="docframe pdfframe" src={pdfSrc + '#navpanes=0&view=FitH'} title={name} allowfullscreen></iframe>
  </div>
{:else if wantPdf && pdfState !== 'fail'}
  <div class="loadwrap"><span class="spin"></span><p class="mnote">กำลังเตรียมเอกสารให้เหมือนต้นฉบับ…</p></div>
{:else if content.kind === 'embed'}
  <img class="docimg" src={embedUrl} alt={name} />
{:else}
  <div class="nopreview">
    <div class="big text-faint flex justify-center"><Icon name="file-text" size={44} stroke={1.5} /></div>
    <p>{wantPdf ? `แสดงเอกสารไม่สำเร็จ (${pdfErr}) — ดาวน์โหลดไฟล์เพื่อเปิดด้วยโปรแกรมในเครื่อง` : content.reason || 'เปิดดูบนหน้าเว็บไม่ได้'}</p>
    {#if downloadUrl}<a class="mbtn inline-flex items-center gap-1.5" href={downloadUrl} download={name}><Icon name="download" size={14} /> ดาวน์โหลดไฟล์</a>{/if}
  </div>
{/if}
