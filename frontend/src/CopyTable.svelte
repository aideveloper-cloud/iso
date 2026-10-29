<script>
  // ตารางสำเนาเอกสาร — ใช้ในหน้าเอกสาร (แท็บสำเนา) และหน้าควบคุมฉบับ / สำเนา
  import { createEventDispatcher } from 'svelte'
  import { copyPdfUrl } from './lib/api.js'
  import { fmtWhen } from './lib/changeNote.js'
  import { COPY_KIND, COPY_STATUS, copyTone } from './lib/copies.js'
  import CopyCloseDialog from './CopyCloseDialog.svelte'
  import Icon from './Icon.svelte'

  export let rows = []
  export let me = null
  /** แสดงคอลัมน์เอกสาร (หน้ารวม) */
  export let showDoc = true
  /** แสดงฉบับปัจจุบันเทียบ (รายการต้องเรียกคืน) */
  export let showCurrent = false
  const dispatch = createEventDispatcher()

  const approver = () => ['approver', 'admin'].includes(me?.role)
  const mine = (c) => c.holderCode && c.holderCode === me?.empCode
  const canClose = (c) => ['active', 'recall'].includes(c.status) && (approver() || mine(c))
  const canPrint = (c) => c.kind === 'print' && c.status === 'active' && (approver() || mine(c))
  let closing = null
</script>

<div class="sec !p-0 overflow-auto">
  <table class="chg-table">
    <thead><tr>
      <th>เลขสำเนา</th>{#if showDoc}<th>เอกสาร</th>{/if}<th>ฉบับ</th>{#if showCurrent}<th>ปัจจุบัน</th>{/if}<th>ประเภท</th><th>ผู้ถือ / จุดใช้งาน</th><th>สถานะ</th><th>ออกเมื่อ</th><th></th>
    </tr></thead>
    <tbody>
      {#each rows as c (c.id)}
        <tr>
          <td class="nowrap"><b>{c.copyNo}</b>{#if c.prints > 1}<div class="mnote !m-0">พิมพ์/ดาวน์โหลด {c.prints} ครั้ง</div>{/if}</td>
          {#if showDoc}<td><button class="code bg-transparent border-0 p-0 cursor-pointer hover:underline" on:click={() => dispatch('open', c.docCode)}>{c.docCode}</button> {c.docName}</td>{/if}
          <td class="nowrap">{c.rev || '—'}</td>
          {#if showCurrent}<td class="nowrap">{c.currentRev || '—'}</td>{/if}
          <td class="nowrap">{COPY_KIND[c.kind] || c.kind}</td>
          <td>{c.holderName || (c.kind === 'print' ? '—' : '')}{#if c.location}<div class="mnote !m-0">{c.location}</div>{/if}</td>
          <td>
            <span class="ia-res {copyTone(c)}">{COPY_STATUS[c.status] || c.status}{#if c.status === 'recall' && c.overdue} · เลยกำหนด{/if}</span>
            {#if c.status === 'recall' && c.recallReason}<div class="mnote !m-0">{c.recallReason}{#if c.days != null} · {c.days} วัน{/if}</div>{/if}
            {#if c.closedAt}<div class="mnote !m-0">{c.closedName} · {fmtWhen(c.closedAt)}{#if c.closedNote} · {c.closedNote}{/if}</div>{/if}
          </td>
          <td class="nowrap">{fmtWhen(c.issuedAt)}{#if c.kind === 'print'}<div class="mnote !m-0">โดย {c.issuedName}</div>{/if}</td>
          <td class="nowrap text-right">
            {#if canPrint(c)}<a class="mbtn gh inline-flex items-center gap-1" href={copyPdfUrl(c.id)} target="_blank" rel="noopener" title="พิมพ์ซ้ำ (นับจำนวนครั้ง)"><Icon name="printer" size={13} /> พิมพ์</a>{/if}
            {#if canClose(c)}<button class="mbtn {c.status === 'recall' ? 'accent' : 'gh'}" on:click={() => (closing = c)}>{c.status === 'recall' ? 'บันทึกเรียกคืน' : 'ปิดสำเนา'}</button>{/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

{#if closing}
  <CopyCloseDialog copy={closing} on:cancel={() => (closing = null)} on:done={(e) => { closing = null; dispatch('changed', e.detail) }} />
{/if}
