<script>
  // ปิดสำเนาเอกสาร: คืน / ทำลาย (ลบไฟล์) / สูญหาย — ผู้ถือหรือผู้ควบคุมเอกสาร
  import { createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { COPY_KIND, closeChoices } from './lib/copies.js'

  export let copy
  const dispatch = createEventDispatcher()
  const choices = closeChoices(copy.kind)
  let result = choices[0][0], note = '', err = '', busy = false

  async function save() {
    if (result === 'lost' && !note.trim()) { err = 'กรุณาระบุรายละเอียดกรณีสูญหาย'; return }
    busy = true; err = ''
    const r = await api.copyClose(copy.id, result, note.trim())
    busy = false
    if (!r.ok) { err = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    dispatch('done', r.body)
  }
</script>

<div class="sgbg" on:mousedown|self={() => dispatch('cancel')}>
  <div class="sgbox">
    <h3>ปิดสำเนา {copy.copyNo}</h3>
    <div class="sgs">{copy.docCode} {copy.rev} · {COPY_KIND[copy.kind]}{#if copy.holderName} · {copy.holderName}{/if}{#if copy.location} · {copy.location}{/if}</div>
    <div class="sgf">
      <label>ผลการเรียกคืน <em>*</em></label>
      <div class="subtabs !mb-0">
        {#each choices as [k, l]}
          <button type="button" class="st-btn {result === k ? 'active' : ''}" on:click={() => (result = k)}>{l}</button>
        {/each}
      </div>
    </div>
    <div class="sgf">
      <label>หมายเหตุ{#if result === 'lost'} <em>*</em>{/if}</label>
      <textarea bind:value={note} rows="2" placeholder={result === 'lost' ? 'เช่น หายระหว่างย้ายจุดงาน — แจ้งหัวหน้างานแล้ว' : ''}></textarea>
    </div>
    {#if err}<div class="merr">{err}</div>{/if}
    <div class="sgact">
      <button class="mbtn gh" on:click={() => dispatch('cancel')}>ยกเลิก</button>
      <button class="mbtn accent" disabled={busy} on:click={save}>{busy ? 'กำลังบันทึก…' : 'บันทึก'}</button>
    </div>
  </div>
</div>
