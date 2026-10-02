<script>
  // ทะเบียนคำขอเปลี่ยนแปลงเอกสาร (DCR) — งานที่รอฉันลงนาม + ติดตามคำขอทั้งหมด
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { DCR_STATUS, fmtWhen } from './lib/changeNote.js'
  import { matchesQuery } from './lib/const.js'
  import Icon from './Icon.svelte'

  export let me = null
  /** เปลี่ยนค่าเมื่อมีการบันทึกที่อื่น เพื่อโหลดรายการใหม่ */
  export let refresh = 0
  const dispatch = createEventDispatcher()

  let list = [], loading = true, err = '', q = ''
  let filter = 'mine'

  async function load() {
    loading = true; err = ''
    try { list = await api.changes() } catch (e) { err = 'โหลดรายการไม่สำเร็จ: ' + e.message }
    loading = false
  }
  onMount(load)
  $: if (refresh) load()

  const isMine = (c) => c.can?.review || c.can?.approve || c.can?.resubmit
  const isOpen = (c) => ['review', 'approve', 'returned'].includes(c.status)
  const FILTERS = [
    ['mine', 'รอฉันดำเนินการ', isMine],
    ['open', 'กำลังดำเนินการ', isOpen],
    ['effective', 'มีผลบังคับใช้', (c) => c.status === 'effective'],
    ['cancelled', 'ยกเลิกคำขอ', (c) => c.status === 'cancelled'],
    ['all', 'ทั้งหมด', () => true],
  ]
  $: counts = Object.fromEntries(FILTERS.map(([k, , fn]) => [k, list.filter(fn).length]))
  $: fn = FILTERS.find(([k]) => k === filter)[2]
  $: rows = list.filter((c) => fn(c) && matchesQuery({ code: c.docCode, name: `${c.docName || ''} ${c.refNo} ${c.preparedName}` }, q))

  function nextStep(c) {
    if (c.can?.review) return 'รอคุณตรวจสอบ'
    if (c.can?.approve) return 'รอคุณอนุมัติ'
    if (c.can?.resubmit) return 'ถูกตีกลับ — รอคุณแก้ไขและส่งใหม่'
    return ''
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="clipboard-check" size={20} /> คำขอเปลี่ยนแปลงเอกสาร (DCR)</h2>
  <p class="cat-desc">เอกสารควบคุมแก้ไขได้ผ่านคำขอเท่านั้น: ผู้จัดทำส่งฉบับร่าง → ผู้ตรวจสอบ → ผู้อนุมัติ → มีผลบังคับใช้เป็น Rev. ใหม่ ทุกขั้นลงนามด้วย PIN (ISO 9001:2015 ข้อ 7.5.2–7.5.3)</p>
</div>

<div class="subtabs">
  {#each FILTERS as [k, label]}
    <button class="st-btn {filter === k ? 'active' : ''}" on:click={() => (filter = k)}>{label} <span class="st-n">{counts[k]}</span></button>
  {/each}
</div>

<div class="controls">
  <div class="search-wrap"><span class="search-ic"><Icon name="search" size={16} /></span><input placeholder="ค้นหา เลขที่ DCR, รหัส หรือชื่อเอกสาร, ผู้จัดทำ..." bind:value={q} /></div>
</div>

{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}
{#if loading}
  <div class="loadwrap"><span class="spin"></span></div>
{:else if !rows.length}
  <div class="empty-state"><Icon name="clipboard-check" size={34} stroke={1.5} /><p>{filter === 'mine' ? 'ไม่มีคำขอที่รอคุณดำเนินการ' : 'ไม่มีคำขอในหมวดนี้'}</p></div>
{:else}
  <div class="sec !p-0 overflow-auto">
    <table>
      <thead><tr><th>เลขที่</th><th>เอกสาร</th><th>Rev.</th><th>เหตุผล</th><th>ผู้จัดทำ</th><th>อัปเดตล่าสุด</th><th>สถานะ</th></tr></thead>
      <tbody>
        {#each rows as c (c.id)}
          <tr class="clickrow" on:click={() => dispatch('open', { code: c.docCode, change: c.id })} title="คลิกเพื่อเปิดคำขอ">
            <td class="code nowrap">{c.refNo}</td>
            <td><b>{c.docCode}</b> {c.docName || ''}</td>
            <td class="nowrap">{c.fromRev || '—'} → <b>{c.newRev}</b></td>
            <td>{c.reason}</td>
            <td>{c.preparedName}</td>
            <td class="nowrap">{fmtWhen(c.updatedAt)}</td>
            <td>
              <span class="status-badge dcr-{c.status}">{DCR_STATUS[c.status] || c.status}</span>
              {#if nextStep(c)}<div class="mut text-[11.5px] mt-1">{nextStep(c)}</div>{/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
