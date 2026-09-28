<script>
  import { createEventDispatcher } from 'svelte'
  import { packChangeNote } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let title = ''
  export let summary = ''
  export let confirmLabel = 'อัปเดทในระบบ'
  /** แสดงฟอร์มประวัติการแก้ไขไฟล์แบบครบ (เรื่อง/รายละเอียด/หน้าที่แก้) */
  export let fullChange = true

  const dispatch = createEventDispatcher()

  let name = '', role = '', topic = '', detail = '', pages = '', chk = false, err = '', busy = false

  function confirm() {
    if (!name.trim()) {
      err = 'กรุณากรอกชื่อผู้แก้ไข'
      return
    }
    if (fullChange && !topic.trim()) {
      err = 'กรุณาระบุเรื่องที่แก้ไข'
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
      signerName: name.trim(),
      signerRole: role.trim(),
      note,
      topic: topic.trim(),
      detail: detail.trim(),
      pages: pages.trim(),
      fail: (m) => {
        err = m
        busy = false
      },
    })
  }
</script>

<div class="sgbg" on:mousedown|self={() => dispatch('cancel')}>
  <div class="sgbox wide">
    <h3>บันทึกการแก้ไขไฟล์</h3>
    <div class="sgs">{title}<br />{summary}</div>

    {#if fullChange}
      <div class="sg-sec">ประวัติการเปลี่ยนแปลง</div>
      <div class="sgf">
        <label>เรื่องที่แก้ไข <em>*</em></label>
        <!-- svelte-ignore a11y-autofocus -->
        <input bind:value={topic} placeholder="เช่น ปรับขั้นตอนการควบคุมเอกสาร" autocomplete="off" autofocus />
      </div>
      <div class="sgf">
        <label>รายละเอียด</label>
        <textarea bind:value={detail} rows="2" placeholder="สรุปสิ่งที่เปลี่ยนในไฟล์"></textarea>
      </div>
      <div class="sgf">
        <label>หน้าที่แก้ไข</label>
        <input bind:value={pages} placeholder="เช่น หน้า 2-3 หรือทั้งฉบับ" />
      </div>
    {/if}

    <div class="sg-sec">ผู้ลงชื่อ</div>
    <div class="sgf">
      <label>ชื่อผู้แก้ไข <em>*</em></label>
      <!-- svelte-ignore a11y-autofocus -->
      <input bind:value={name} placeholder="ชื่อ-สกุล" autocomplete="off" autofocus={!fullChange} />
    </div>
    <div class="sgf">
      <label>ตำแหน่ง / แผนก</label>
      <input bind:value={role} placeholder="เช่น QMR, DCC, หัวหน้าฝ่าย" />
    </div>
    {#if !fullChange}
      <div class="sgf">
        <label>สรุปการเปลี่ยนแปลง</label>
        <textarea bind:value={detail} rows="2" placeholder="แก้ไขอะไรบ้าง"></textarea>
      </div>
    {/if}

    <label class="sgchk">
      <input type="checkbox" bind:checked={chk} />
      <span>ข้าพเจ้ายืนยันว่าการแก้ไขไฟล์นี้ถูกต้อง และบันทึกเข้าสู่ระบบควบคุมเอกสารแล้ว</span>
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
