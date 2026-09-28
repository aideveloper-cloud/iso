<script>
  import { noteParts, changeTopic, fmtDate } from './lib/changeNote.js'

  /** @type {Array<{ version: number, source?: string, note?: string, signedAt?: string, signerName?: string, signerRole?: string }>} */
  export let versions = []
  /** เวอร์ชันที่กำลังเปิดดู — ไฮไลต์แถว */
  export let currentVer = null
  export let showSigner = false
  export let onDoc = false
</script>

{#if versions.length}
  <div class="chg-wrap" class:on-doc={onDoc}>
    <h3 class="chg-title">ประวัติการแก้ไขเพิ่มเติม / เปลี่ยนแปลง</h3>
    <table class="chg-table">
      <thead>
        <tr>
          <th>ลำดับ</th>
          <th>ฉบับ</th>
          <th>เรื่อง</th>
          <th>รายละเอียด</th>
          <th>หน้าที่แก้ไข</th>
          <th>วันที่บังคับใช้</th>
          {#if showSigner}<th>ผู้แก้ไข</th>{/if}
          {#if $$slots.actions}<th></th>{/if}
        </tr>
      </thead>
      <tbody>
        {#each versions as v, i}
          {@const np = noteParts(v)}
          <tr class={currentVer != null && v.version === currentVer ? 'cur' : ''}>
            <td>{versions.length - i}</td>
            <td><span class="verpill">v{v.version}</span></td>
            <td>{changeTopic(v, np)}</td>
            <td>{np.detail || np.extra || '—'}</td>
            <td>{np.pages || '—'}</td>
            <td>{fmtDate(v.signedAt)}</td>
            {#if showSigner}
              <td>{v.signerName}{#if v.signerRole}<br /><span class="mut">{v.signerRole}</span>{/if}</td>
            {/if}
            {#if $$slots.actions}
              <td class="nowrap"><slot name="actions" {v} /></td>
            {/if}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
