<script>
  import { createEventDispatcher } from 'svelte'
  const dispatch = createEventDispatcher()

  function run(cmd) {
    dispatch('exec', { cmd })
  }
</script>

<!-- ปุ่มเฉพาะที่บันทึกกลับลงไฟล์ Word เดิมได้ครบ: ตัวหนา/เอียง/ขีดเส้นใต้ และเพิ่ม-ลบแถวตาราง -->
<div class="rte-toolbar">
  <button type="button" on:mousedown|preventDefault on:click={() => run('bold')} title="ตัวหนา"><b>B</b></button>
  <button type="button" on:mousedown|preventDefault on:click={() => run('italic')} title="ตัวเอียง"><i>I</i></button>
  <button type="button" on:mousedown|preventDefault on:click={() => run('underline')} title="ขีดเส้นใต้"><u>U</u></button>
  <span class="rte-sep"></span>
  <button type="button" on:mousedown|preventDefault on:click={() => dispatch('row', 'add')} title="เพิ่มแถวใต้แถวที่เคอร์เซอร์อยู่ (ในตาราง)">+ แถว</button>
  <button type="button" on:mousedown|preventDefault on:click={() => dispatch('row', 'del')} title="ลบแถวที่เคอร์เซอร์อยู่ (ในตาราง)">− แถว</button>
</div>
