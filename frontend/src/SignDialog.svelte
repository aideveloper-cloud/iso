<script>
  import { createEventDispatcher } from 'svelte'
  import Icon from './Icon.svelte'
  export let title = ''
  export let summary = ''
  const dispatch = createEventDispatcher()

  let name = '', role = '', note = '', chk = false, err = '', busy = false

  async function confirm() {
    if (!name.trim()) { err = 'กรุณากรอกชื่อผู้แก้ไข'; return }
    if (!chk) { err = 'กรุณาติ๊กยืนยันความถูกต้องก่อนบันทึก'; return }
    err = ''; busy = true
    dispatch('confirm', { signerName: name.trim(), signerRole: role.trim(), note: note.trim(), fail: (m) => { err = m; busy = false } })
  }
</script>

<div class="sgbg" on:mousedown|self={() => dispatch('cancel')}>
  <div class="sgbox">
    <h3>ลงชื่อผู้แก้ไข</h3>
    <div class="sgs">{title}<br />{summary}</div>
    <div class="sgf"><label>ชื่อผู้แก้ไข <em>*</em></label>
      <!-- svelte-ignore a11y-autofocus -->
      <input bind:value={name} placeholder="ชื่อ-สกุล" autocomplete="off" autofocus /></div>
    <div class="sgf"><label>ตำแหน่ง / แผนก</label>
      <input bind:value={role} placeholder="เช่น QMR, หัวหน้าฝ่ายผลิต" /></div>
    <div class="sgf"><label>สรุปการเปลี่ยนแปลง</label>
      <textarea bind:value={note} rows="2" placeholder="แก้ไขอะไรบ้าง"></textarea></div>
    <label class="sgchk"><input type="checkbox" bind:checked={chk} />
      <span>ข้าพเจ้ายืนยันว่าการแก้ไขนี้ถูกต้องและได้รับอนุมัติแล้ว</span></label>
    {#if err}<div class="merr">{err}</div>{/if}
    <div class="sgact">
      <button class="mbtn gh" on:click={() => dispatch('cancel')}>ยกเลิก</button>
      <button class="mbtn accent inline-flex items-center gap-1.5" disabled={busy} on:click={confirm}>{#if busy}กำลังบันทึก…{:else}<Icon name="save" size={14} /> บันทึกเวอร์ชันใหม่{/if}</button>
    </div>
  </div>
</div>
