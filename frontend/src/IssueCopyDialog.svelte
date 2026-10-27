<script>
  // ออกสำเนาควบคุม (ผู้ควบคุมเอกสาร) — ได้เลขสำเนา CC-<พ.ศ.>-<ลำดับ> แล้วเปิด PDF ที่ประทับเลขสำเนา + QR ไปสั่งพิมพ์
  import { createEventDispatcher } from 'svelte'
  import { api, copyPdfUrl } from './lib/api.js'
  import Icon from './Icon.svelte'

  export let doc
  export let rev = ''
  const dispatch = createEventDispatcher()
  let location = '', holderCode = '', holderName = '', note = '', err = '', busy = false
  let issued = null

  async function lookup() {
    holderName = ''
    const code = holderCode.trim()
    if (!code) return
    const list = await api.employees(code).catch(() => [])
    holderName = list.find((e) => e.empCode === code)?.nameTh || ''
    if (!holderName) err = `ไม่พบพนักงานรหัส ${code}`
    else err = ''
  }

  async function issue() {
    if (!location.trim()) { err = 'กรุณาระบุจุดใช้งาน/ที่เก็บสำเนา'; return }
    busy = true; err = ''
    const r = await api.copyIssue(doc.code, { location: location.trim(), holderCode: holderCode.trim(), note: note.trim() })
    busy = false
    if (!r.ok) { err = r.body.error || 'ออกสำเนาไม่สำเร็จ'; return }
    issued = r.body
    dispatch('issued', issued)
  }
</script>

<div class="sgbg" on:mousedown|self={() => dispatch('close')}>
  <div class="sgbox">
    {#if !issued}
      <h3>ออกสำเนาควบคุม</h3>
      <div class="sgs">{doc.code} · {doc.name} · {rev} — ทุกหน้าประทับเลขสำเนา + QR ตรวจ Rev. ปัจจุบัน · เมื่อออก Rev. ใหม่ ระบบจะแจ้งให้เรียกคืนสำเนานี้</div>
      <div class="sgf">
        <label>จุดใช้งาน / ที่เก็บสำเนา <em>*</em></label>
        <input bind:value={location} placeholder="เช่น โรงงาน · เครื่องตัดลวด, โกดัง, ห้อง QC" />
      </div>
      <div class="sgf">
        <label>ผู้ถือสำเนา (รหัสพนักงาน)</label>
        <input bind:value={holderCode} on:blur={lookup} placeholder="ไม่บังคับ — ผู้ถือจะได้รับงานคืนสำเนาเมื่อออก Rev. ใหม่" />
        {#if holderName}<span class="mnote !m-0">{holderName}</span>{/if}
      </div>
      <div class="sgf">
        <label>หมายเหตุ</label>
        <input bind:value={note} placeholder="เช่น ติดที่บอร์ดหน้างาน" />
      </div>
      {#if err}<div class="merr">{err}</div>{/if}
      <div class="sgact">
        <button class="mbtn gh" on:click={() => dispatch('close')}>ยกเลิก</button>
        <button class="mbtn accent inline-flex items-center gap-1.5" disabled={busy} on:click={issue}><Icon name="printer" size={14} /> {busy ? 'กำลังออกเลข…' : 'ออกเลขสำเนา'}</button>
      </div>
    {:else}
      <h3>ออกสำเนา {issued.copyNo} แล้ว</h3>
      <div class="sgs">{issued.docCode} {issued.rev} · {issued.location}{#if issued.holderName} · {issued.holderName}{/if}</div>
      <p class="text-[13px] m-0 mb-3">เปิดไฟล์แล้วสั่งพิมพ์จากหน้าต่างใหม่ — ทุกหน้ามีเลขสำเนาและ QR สำหรับสแกนตรวจว่ายังเป็นฉบับปัจจุบัน</p>
      <div class="sgact">
        <button class="mbtn gh" on:click={() => dispatch('close')}>ปิด</button>
        <a class="mbtn accent inline-flex items-center gap-1.5" href={copyPdfUrl(issued.id)} target="_blank" rel="noopener"><Icon name="printer" size={14} /> เปิดไฟล์เพื่อพิมพ์</a>
      </div>
    {/if}
  </div>
</div>
