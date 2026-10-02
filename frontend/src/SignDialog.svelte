<script>
  import { createEventDispatcher } from 'svelte'
  import { packChangeNote, parseChangeNote } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let title = ''
  export let summary = ''
  export let confirmLabel = 'อัปเดทในระบบ'
  /** แสดงฟอร์มประวัติการแก้ไขไฟล์แบบครบ (เรื่อง/รายละเอียด/หน้าที่แก้) */
  export let fullChange = true
  /** ผู้ใช้ที่ login — เป็นผู้ลงนามเสมอ */
  export let me = null
  /** ต้องกรอก PIN ซ้ำเพื่อลงนามอิเล็กทรอนิกส์ */
  export let withPin = false
  /** คำขอเปลี่ยนแปลงเอกสาร: { fromRev, newRev } — แสดงช่องเหตุผลและ Rev. ใหม่ */
  export let change = null
  /** ค่าเริ่มต้น (ตอนส่งใหม่หลังถูกตีกลับ) */
  export let initialNote = ''
  export let initialReason = ''

  const dispatch = createEventDispatcher()

  const init = parseChangeNote(initialNote)
  let topic = init.topic, detail = init.detail, pages = init.pages
  let reason = initialReason
  let newRev = change?.newRev || ''
  let pin = '', chk = false, err = '', busy = false

  function confirm() {
    if (change && !reason.trim()) {
      err = 'กรุณาระบุเหตุผลการเปลี่ยนแปลง'
      return
    }
    if (change && !newRev.trim()) {
      err = 'กรุณาระบุ Rev. ใหม่'
      return
    }
    if (fullChange && !topic.trim()) {
      err = 'กรุณาระบุเรื่องที่แก้ไข'
      return
    }
    if (withPin && !/^\d{6}$/.test(pin.trim())) {
      err = 'กรุณากรอก PIN 6 หลักเพื่อยืนยันการลงนาม'
      return
    }
    if (!chk) {
      err = 'กรุณาติ๊กยืนยันความถูกต้องก่อนบันทึก'
      return
    }
    err = ''
    busy = true
    const note = fullChange
      ? packChangeNote({ topic, detail, pages })
      : detail.trim() || topic.trim()
    dispatch('confirm', {
      note,
      reason: reason.trim(),
      newRev: newRev.trim(),
      pin: pin.trim(),
      fail: (m) => {
        err = m
        busy = false
      },
    })
  }
</script>

<div class="sgbg" on:mousedown|self={() => dispatch('cancel')}>
  <div class="sgbox wide">
    <h3>{change ? 'ส่งคำขอเปลี่ยนแปลงเอกสาร (DCR)' : 'บันทึกการแก้ไขไฟล์'}</h3>
    <div class="sgs">{title}<br />{summary}</div>

    {#if change}
      <div class="sg-sec">คำขอเปลี่ยนแปลง</div>
      <div class="sgf">
        <label>เหตุผลการเปลี่ยนแปลง <em>*</em></label>
        <!-- svelte-ignore a11y-autofocus -->
        <textarea bind:value={reason} rows="2" placeholder="เช่น ปรับตามผลตรวจติดตามภายใน / CAR-2569-003 / เปลี่ยนขั้นตอนการทำงาน" autofocus></textarea>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ฉบับปัจจุบัน</label><input value={change.fromRev || '—'} disabled /></div>
        <div class="sgf"><label>Rev. ใหม่ <em>*</em></label><input bind:value={newRev} placeholder="เช่น Rev.01" /></div>
      </div>
    {/if}

    {#if fullChange}
      <div class="sg-sec">ประวัติการเปลี่ยนแปลง</div>
      <div class="sgf">
        <label>เรื่องที่แก้ไข <em>*</em></label>
        <!-- svelte-ignore a11y-autofocus -->
        <input bind:value={topic} placeholder="เช่น ปรับขั้นตอนการควบคุมเอกสาร" autocomplete="off" autofocus={!change} />
      </div>
      <div class="sgf">
        <label>รายละเอียด</label>
        <textarea bind:value={detail} rows="2" placeholder="สรุปสิ่งที่เปลี่ยนในไฟล์"></textarea>
      </div>
      <div class="sgf">
        <label>หน้าที่แก้ไข</label>
        <input bind:value={pages} placeholder="เช่น หน้า 2-3 หรือทั้งฉบับ" />
      </div>
    {:else}
      <div class="sgf">
        <label>สรุปการเปลี่ยนแปลง</label>
        <textarea bind:value={detail} rows="2" placeholder="แก้ไขอะไรบ้าง"></textarea>
      </div>
    {/if}

    <div class="sg-sec">ผู้ลงนาม</div>
    <div class="sg-signer inline-flex items-center gap-1.5"><Icon name="pen-line" size={14} /> <b>{me?.name || '—'}</b>{#if me?.position} · {me.position}{/if}</div>
    {#if withPin}
      <div class="sgf">
        <label>PIN ยืนยันการลงนาม <em>*</em></label>
        <input bind:value={pin} type="password" maxlength="6" inputmode="numeric" autocomplete="current-password" placeholder="PIN 6 หลัก" />
      </div>
    {/if}

    <label class="sgchk">
      <input type="checkbox" bind:checked={chk} />
      <span>{change
        ? 'ข้าพเจ้ายืนยันว่าฉบับร่างนี้ถูกต้อง และขอส่งให้ผู้ตรวจสอบ/ผู้อนุมัติพิจารณาก่อนมีผลบังคับใช้'
        : 'ข้าพเจ้ายืนยันว่าการแก้ไขไฟล์นี้ถูกต้อง และบันทึกเข้าสู่ระบบควบคุมเอกสารแล้ว'}</span>
    </label>
    {#if err}<div class="merr">{err}</div>{/if}
    <div class="sgact">
      <button class="mbtn gh" on:click={() => dispatch('cancel')}>ยกเลิก</button>
      <button class="mbtn accent inline-flex items-center gap-1.5" disabled={busy} on:click={confirm}>
        {#if busy}กำลังบันทึก…{:else}<Icon name="save" size={14} /> {confirmLabel}{/if}
      </button>
    </div>
  </div>
</div>
