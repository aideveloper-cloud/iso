<script>
  import { createEventDispatcher } from 'svelte'
  export let title = 'ยืนยันการทำรายการ'
  export let message = ''
  export let confirmLabel = 'ยืนยัน'
  export let cancelLabel = 'ยกเลิก'
  export let danger = false
  const dispatch = createEventDispatcher()
  const confirm = () => dispatch('confirm')
  const cancel = () => dispatch('cancel')
  function onKey(e) { if (e.key === 'Escape') cancel() }
</script>

<svelte:window on:keydown={onKey} />
<div class="sgbg z-[200]" on:mousedown|self={cancel}>
  <div class="sgbox">
    <h3>{title}</h3>
    <p class="sgs">{message}</p>
    <div class="sgact">
      <button class="mbtn gh" on:click={cancel}>{cancelLabel}</button>
      <button class="mbtn {danger ? 'danger' : ''}" on:click={confirm}>{confirmLabel}</button>
    </div>
  </div>
</div>
