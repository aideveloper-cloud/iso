<script>
  // ลงนามตัดสินใจ (ผ่าน/อนุมัติ/ตีกลับ/ยกเลิกใช้) — ผู้ลงนามคือผู้ใช้ที่ login และต้องกรอก PIN ซ้ำ
  import { createEventDispatcher } from 'svelte'
  import Icon from './Icon.svelte'

  export let title = ''
  export let message = ''
  export let me = null
  export let commentLabel = 'ความเห็น'
  export let commentRequired = false
  export let withPin = true
  export let confirmLabel = 'ยืนยัน'
  export let danger = false
  /** ข้อความเริ่มต้นในช่องความเห็น (เช่น ข้อแนะนำ AI ที่ผู้ตรวจเลือกทำตาม) — แก้ได้ก่อนลงนาม */
  export let initialComment = ''

  const dispatch = createEventDispatcher()
  let comment = initialComment, pin = '', err = '', busy = false

  function confirm() {
    if (commentRequired && !comment.trim()) { err = `กรุณาระบุ${commentLabel}`; return }
    if (withPin && !/^\d{6}$/.test(pin.trim())) { err = 'กรุณากรอก PIN 6 หลักเพื่อยืนยันการลงนาม'; return }
    err = ''
    busy = true
    dispatch('confirm', {
      comment: comment.trim(),
      pin: pin.trim(),
      fail: (m) => { err = m; busy = false },
    })
  }
</script>

<div class="sgbg" on:mousedown|self={() => dispatch('cancel')}>
  <div class="sgbox">
    <h3>{title}</h3>
    {#if message}<div class="sgs">{message}</div>{/if}
    <div class="sgf">
      <label>{commentLabel}{#if commentRequired} <em>*</em>{/if}</label>
      <!-- svelte-ignore a11y-autofocus -->
      <textarea bind:value={comment} rows="3" autofocus></textarea>
    </div>
    <div class="sg-signer inline-flex items-center gap-1.5"><Icon name="pen-line" size={14} /> <b>{me?.name || '—'}</b>{#if me?.position} · {me.position}{/if}</div>
    {#if withPin}
      <div class="sgf">
        <label>PIN ยืนยันการลงนาม <em>*</em></label>
        <input bind:value={pin} type="password" maxlength="6" inputmode="numeric" autocomplete="current-password" placeholder="PIN 6 หลัก"
          on:keydown={(e) => e.key === 'Enter' && confirm()} />
      </div>
    {/if}
    {#if err}<div class="merr">{err}</div>{/if}
    <div class="sgact">
      <button class="mbtn gh" on:click={() => dispatch('cancel')}>ยกเลิก</button>
      <button class="mbtn {danger ? 'danger' : 'accent'}" disabled={busy} on:click={confirm}>{busy ? 'กำลังบันทึก…' : confirmLabel}</button>
    </div>
  </div>
</div>
