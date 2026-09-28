<script>
  import { createEventDispatcher } from 'svelte'
  const dispatch = createEventDispatcher()

  function run(cmd, val) {
    dispatch('exec', { cmd, val })
  }
</script>

<div class="rte-toolbar">
  <button type="button" on:click={() => run('bold')} title="ตัวหนา"><b>B</b></button>
  <button type="button" on:click={() => run('italic')} title="ตัวเอียง"><i>I</i></button>
  <button type="button" on:click={() => run('underline')} title="ขีดเส้นใต้"><u>U</u></button>
  <span class="rte-sep"></span>
  <button type="button" on:click={() => run('formatBlock', 'H2')} title="หัวข้อ">H2</button>
  <button type="button" on:click={() => run('formatBlock', 'H3')} title="หัวข้อย่อย">H3</button>
  <button type="button" on:click={() => run('formatBlock', 'P')} title="ย่อหน้าปกติ">P</button>
  <span class="rte-sep"></span>
  <button type="button" on:click={() => run('insertUnorderedList')} title="รายการ">•</button>
  <button type="button" on:click={() => run('insertOrderedList')} title="รายการเลข">1.</button>
  <span class="rte-sep"></span>
  <button type="button" on:click={() => run('removeFormat')} title="ล้างรูปแบบ">✕ฟอร์แมต</button>
</div>
