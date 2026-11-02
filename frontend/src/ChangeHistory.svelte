<script>
  import { noteParts, changeTopic, fmtDate } from './lib/changeNote.js'

  /** @type {Array<{ version: number, source?: string, note?: string, signedAt?: string, signerName?: string, signerRole?: string }>} */
  export let versions = []
  /** เวอร์ชันที่กำลังเปิดดู — ไฮไลต์แถว */
  export let currentVer = null
  export let showSigner = false
  export let onDoc = false
  /** เอกสารควบคุมในทะเบียน — แสดงฉบับ/ผู้อนุมัติ และติดป้ายเวอร์ชันที่บันทึกก่อนมีระบบอนุมัติ */
  export let controlled = false

  const legacy = (v) => controlled && v.source !== 'original' && !v.changeId
  const effDate = (v) => v.effectiveDate || fmtDate(v.signedAt)
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
          {#if showSigner}<th>ผู้จัดทำ</th>{#if controlled}<th>ผู้อนุมัติ</th>{/if}{/if}
          {#if $$slots.actions}<th></th>{/if}
        </tr>
      </thead>
      <tbody>
        {#each versions as v, i}
          {@const np = noteParts(v)}
          <tr class={currentVer != null && v.version === currentVer ? 'cur' : ''}>
            <td>{versions.length - i}</td>
            <td><span class="verpill">#{v.version}</span>{#if v.rev} <span class="nowrap">{v.rev}</span>{/if}</td>
            <td>{changeTopic(v, np)}{#if legacy(v)}<br /><span class="legacy-flag" title="บันทึกก่อนเปิดใช้ขั้นตอนตรวจสอบ/อนุมัติ (DCR)">ไม่ผ่าน DCR</span>{/if}</td>
            <td>{np.detail || np.extra || '—'}</td>
            <td>{np.pages || '—'}</td>
            <td>{effDate(v)}</td>
            {#if showSigner}
              <td>{v.signerName}{#if v.signerRole}<br /><span class="mut">{v.signerRole}</span>{/if}</td>
              {#if controlled}<td>{v.approvedBy || '—'}</td>{/if}
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
