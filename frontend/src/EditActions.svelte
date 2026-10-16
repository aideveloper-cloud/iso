<script>
  import { createEventDispatcher } from 'svelte'
  import Icon from './Icon.svelte'

  export let editing = false
  export let dirty = false
  /** ป้ายปุ่มตอนยังไม่เข้าโหมดแก้ */
  export let editLabel = 'แก้ไขไฟล์'
  /** ใช้สี accent กับปุ่มเริ่มแก้ไข (ในแผง file-tools) */
  export let accent = false
  /** ป้ายปุ่มบันทึกตอนอยู่ในโหมดแก้ */
  export let updateLabel = 'อัปเดทเข้าสู่ระบบ'

  const dispatch = createEventDispatcher()
</script>

{#if editing}
  <button class="mbtn gh" on:click={() => dispatch('cancel')}>ยกเลิกแก้ไข</button>
  <button class="mbtn accent inline-flex items-center gap-1.5" disabled={!dirty} on:click={() => dispatch('update')}>
    <Icon name="save" size={14} /> {updateLabel}
  </button>
{:else}
  <button
    class="mbtn inline-flex items-center gap-1.5"
    class:accent
    on:click={() => dispatch('start')}
  >
    <Icon name="pencil" size={14} /> {editLabel}
  </button>
{/if}
