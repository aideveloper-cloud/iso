<script>
  // การรับทราบเอกสาร (7.5.3, 7.3) — ของฉัน: เอกสารที่ต้องอ่านและรับทราบ · ภาพรวม (ผู้อนุมัติขึ้นไป): อัตรารับทราบ เลยกำหนด ส่งคำขอทีละหลายฉบับ
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import { fmtWhen, fmtDate } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let me = null
  export let docs = []
  export let refresh = 0
  const dispatch = createEventDispatcher()

  const approver = () => ['approver', 'admin'].includes(me?.role)
  let view = 'mine', mine = [], ov = null, err = '', msg = '', filter = 'pending', busy = false
  let bulkType = '', bulkDept = ''

  async function load() {
    err = ''
    try {
      mine = await api.acksMine()
      if (approver()) ov = await api.acksOverview()
    } catch (e) { err = 'โหลดไม่สำเร็จ: ' + e.message }
  }
  onMount(load)
  $: if (refresh) load()

  $: pending = mine.filter((a) => a.status === 'pending')
  $: history = mine.filter((a) => a.status !== 'pending')
  $: list = ov ? ov.acks.filter((a) => filter === 'all' || (filter === 'overdue' ? a.overdue : a.status === filter)) : []
  $: bulkCodes = docs.filter((d) => d.docStatus !== 'obsolete' && !d.missing && (!bulkType || d.type === bulkType) && (!bulkDept || d.dept === bulkDept)).map((d) => d.code)

  async function bulk() {
    if (!bulkCodes.length) return
    busy = true; err = ''
    const r = await api.acksBulk(bulkCodes)
    busy = false
    if (!r.ok) { err = r.body.error || 'ส่งไม่สำเร็จ'; return }
    msg = `ส่งคำขอรับทราบ ${r.body.docs} ฉบับ รวม ${r.body.requests} คำขอ (คนที่ได้รับฉบับนี้แล้วไม่ถูกส่งซ้ำ)`
    load()
  }
  function exportCsv() {
    const cell = (v) => { let x = String(v ?? ''); if (/^[\s]*[=+\-@\t\r]/.test(x)) x = "'" + x; return '"' + x.replace(/"/g, '""') + '"' }
    const rows = [['เอกสาร', 'ชื่อเอกสาร', 'ฉบับ', 'พนักงาน', 'รหัส', 'แผนก', 'สถานะ', 'ครบกำหนด', 'เปิดอ่าน', 'รับทราบ'],
      ...ov.acks.map((a) => [a.docCode, a.docName, a.rev, a.empName, a.empCode, a.dept, a.overdue ? 'เลยกำหนด' : a.status === 'acknowledged' ? 'รับทราบแล้ว' : 'รอรับทราบ', a.dueDate, a.openedAt ? fmtWhen(a.openedAt) : '', a.ackedAt ? fmtWhen(a.ackedAt) : ''])]
    const url = URL.createObjectURL(new Blob(['﻿' + rows.map((r) => r.map(cell).join(',')).join('\r\n')], { type: 'text/csv;charset=utf-8' }))
    const el = document.createElement('a'); el.href = url; el.download = 'document-acknowledgement.csv'; el.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="clipboard-check" size={20} /> การรับทราบเอกสาร</h2>
  <p class="cat-desc">ISO 9001:2015 ข้อ 7.5.3 / 7.3 — เมื่อเอกสารออกฉบับใหม่ ผู้เกี่ยวข้องต้องเปิดอ่านและลงนาม “อ่านและเข้าใจแล้ว” ด้วย PIN · บันทึกเป็นหลักฐานในประวัติการอบรม</p>
</div>

{#if approver()}
  <div class="subtabs">
    <button class="st-btn {view === 'mine' ? 'active' : ''}" on:click={() => (view = 'mine')}>ของฉัน <span class="st-n">{pending.length}</span></button>
    <button class="st-btn {view === 'all' ? 'active' : ''}" on:click={() => (view = 'all')}>ภาพรวมทั้งบริษัท</button>
  </div>
{/if}
{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}
{#if msg}<div class="save-ok">{msg}</div>{/if}

{#if view === 'mine'}
  {#if !pending.length}
    <div class="empty-state"><Icon name="circle-check" size={34} stroke={1.5} /><p>ไม่มีเอกสารที่ต้องรับทราบ</p></div>
  {:else}
    <div class="act-grid">
      {#each pending as a (a.id)}
        <div class="act-card" role="button" tabindex="0" on:click={() => dispatch('open', a.docCode)} on:keydown={(e) => e.key === 'Enter' && dispatch('open', a.docCode)}>
          <div class="ac-top"><span class="ac-ref">{a.docCode} · {a.rev}</span><span class="flex-1"></span><span class="ia-res {a.overdue ? 'r-major' : 'r-minor'}">{a.overdue ? 'เลยกำหนด' : 'รอรับทราบ'}</span></div>
          <div class="ac-title">{a.docName}</div>
          <div class="ac-meta"><span>ครบกำหนด {a.dueDate ? fmtDate(a.dueDate) : '—'}</span><span>ส่งโดย {a.requestedBy}</span>{#if a.openedAt}<span>เปิดอ่านแล้ว</span>{/if}</div>
          <div class="mnote !m-0">คลิกเพื่อเปิดอ่าน แล้วกด “อ่านและเข้าใจแล้ว” ในหน้าเอกสาร</div>
        </div>
      {/each}
    </div>
  {/if}
  {#if history.length}
    <h3 class="rd-sub">ประวัติการรับทราบของฉัน</h3>
    <table class="chg-table"><thead><tr><th>เอกสาร</th><th>ฉบับ</th><th>สถานะ</th><th>รับทราบเมื่อ</th></tr></thead>
      <tbody>{#each history as a}<tr><td><b>{a.docCode}</b> {a.docName}</td><td>{a.rev}</td><td>{a.status === 'acknowledged' ? 'รับทราบแล้ว' : 'ถูกแทนที่ด้วยฉบับใหม่'}</td><td class="nowrap">{a.ackedAt ? fmtWhen(a.ackedAt) : '—'}</td></tr>{/each}</tbody></table>
  {/if}
{:else if ov}
  <div class="rd-metrics">
    <div class="rd-metric"><div class="rd-ml">อัตรารับทราบ<Icon name="circle-check" size={18} /></div><div class="rd-mv">{ov.summary.rate ?? 0}%</div><div class="rd-mn"><i class="dot {ov.summary.rate === 100 ? 'ok' : 'warn'}"></i>{ov.summary.acknowledged}/{ov.summary.total} คำขอ</div></div>
    <div class="rd-metric"><div class="rd-ml">รอรับทราบ<Icon name="clock" size={18} /></div><div class="rd-mv">{ov.summary.pending}</div><div class="rd-mn"><i class="dot {ov.summary.pending ? 'warn' : 'ok'}"></i>คำขอที่ยังไม่รับทราบ</div></div>
    <div class="rd-metric"><div class="rd-ml">เลยกำหนด<Icon name="alert-triangle" size={18} /></div><div class="rd-mv">{ov.summary.overdue}</div><div class="rd-mn"><i class="dot {ov.summary.overdue ? 'bad' : 'ok'}"></i>{ov.summary.overdueDocs.length} เอกสาร</div></div>
    <div class="rd-metric"><div class="rd-ml">เอกสารที่ส่งให้รับทราบ<Icon name="files" size={18} /></div><div class="rd-mv">{ov.docs.length}</div><div class="rd-mn"><i class="dot {ov.docs.length ? 'ok' : 'warn'}"></i>จาก {docs.length} ฉบับในทะเบียน</div></div>
  </div>

  <div class="sec mb-3">
    <h2 class="!mb-2">ส่งคำขอรับทราบฉบับปัจจุบันทีละหลายฉบับ</h2>
    <div class="flex flex-wrap items-center gap-2">
      <select class="ia-year" bind:value={bulkType}><option value="">ทุกประเภท</option>{#each ['QM', 'QP', 'WI', 'FM'] as t}<option value={t}>{t}</option>{/each}</select>
      <select class="ia-year" bind:value={bulkDept}><option value="">ทุกแผนก</option>{#each DEPTS as d}<option value={d}>{DEPTNAME[d]}</option>{/each}</select>
      <button class="mbtn accent" disabled={busy || !bulkCodes.length} on:click={bulk}>{busy ? 'กำลังส่ง…' : `ส่งให้รับทราบ ${bulkCodes.length} ฉบับ`}</button>
      <span class="mnote !m-0">ส่งตามรายชื่อแจกจ่ายของแต่ละเอกสาร · คนที่มีคำขอฉบับนั้นอยู่แล้วไม่ถูกส่งซ้ำ</span>
    </div>
  </div>

  {#if ov.docs.length}
    <div class="sec !p-0 overflow-auto mb-3">
      <table>
        <thead><tr><th>เอกสาร</th><th>ฉบับ</th><th>รับทราบ</th><th>เลยกำหนด</th><th>ความคืบหน้า</th></tr></thead>
        <tbody>
          {#each ov.docs as d}
            <tr class="clickrow" on:click={() => dispatch('open', d.docCode)}>
              <td><b class="code">{d.docCode}</b> {d.docName}</td><td>{d.rev}</td><td>{d.acknowledged}/{d.total}</td><td class:text-danger={d.overdue}>{d.overdue || '—'}</td>
              <td style="min-width:140px"><div class="prog"><div class="prog-fill" style="width:{d.rate}%"></div></div><div class="ac-prog-n">{d.rate}%</div></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="flex flex-wrap items-center gap-2 mb-2">
      <div class="subtabs !mb-0">{#each [['pending', 'รอรับทราบ'], ['overdue', 'เลยกำหนด'], ['acknowledged', 'รับทราบแล้ว'], ['all', 'ทั้งหมด']] as [k, l]}<button class="st-btn {filter === k ? 'active' : ''}" on:click={() => (filter = k)}>{l}</button>{/each}</div>
      <span class="flex-1"></span>
      <button class="mbtn gh inline-flex items-center gap-1.5" on:click={exportCsv}><Icon name="download" size={14} /> CSV</button>
    </div>
    <div class="sec !p-0 overflow-auto">
      <table class="chg-table"><thead><tr><th>เอกสาร</th><th>พนักงาน</th><th>แผนก</th><th>สถานะ</th><th>ครบกำหนด</th><th>รับทราบ</th></tr></thead>
        <tbody>{#each list.slice(0, 500) as a (a.id)}<tr><td class="nowrap"><b>{a.docCode}</b> {a.rev}</td><td>{a.empName}</td><td>{DEPTNAME[a.dept] || a.dept}</td>
          <td><span class="ia-res {a.status === 'acknowledged' ? 'r-conform' : a.overdue ? 'r-major' : 'r-minor'}">{a.overdue ? 'เลยกำหนด' : a.status === 'acknowledged' ? 'รับทราบแล้ว' : 'รอรับทราบ'}</span></td>
          <td class="nowrap">{a.dueDate ? fmtDate(a.dueDate) : '—'}</td><td class="nowrap">{a.ackedAt ? fmtWhen(a.ackedAt) : '—'}</td></tr>{/each}</tbody></table>
    </div>
  {:else}
    <div class="empty-state"><Icon name="clipboard-check" size={34} stroke={1.5} /><p>ยังไม่เคยส่งคำขอรับทราบ — ใช้ปุ่มด้านบนเพื่อเริ่ม</p></div>
  {/if}
{/if}
