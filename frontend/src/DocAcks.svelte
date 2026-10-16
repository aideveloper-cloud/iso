<script>
  // การแจกจ่ายและการรับทราบของเอกสารหนึ่งฉบับ (7.5.3) — รายชื่อแจกจ่าย · ส่งคำขอ · ใครรับทราบเมื่อไร
  import { createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import { ROLES } from './lib/mreview.js'
  import { fmtWhen, fmtDate } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let data // ผลจาก /api/docs/:code/acks
  export let code
  const dispatch = createEventDispatcher()

  const STATUS = { pending: 'รอรับทราบ', acknowledged: 'รับทราบแล้ว', superseded: 'ถูกแทนที่ (Rev. ใหม่)' }
  let editing = false, draft = [], err = '', msg = '', busy = false, showOld = false, people = [], pq = ''

  $: cur = data.acks.filter((a) => a.version === data.currentVersion)
  $: old = data.acks.filter((a) => a.version !== data.currentVersion)
  $: done = cur.filter((a) => a.status === 'acknowledged').length
  $: notRequested = data.people.filter((p) => !cur.some((a) => a.empCode === p.code))
  const label = (t) => t.type === 'all' ? 'ทุกคน' : t.type === 'dept' ? 'แผนก ' + (DEPTNAME[t.value] || t.value) : t.type === 'role' ? 'บทบาท ' + (ROLES[t.value] || t.value) : 'พนักงาน ' + t.value

  function startEdit() { draft = data.targets.map((t) => ({ ...t })); editing = true; err = '' }
  const has = (type, value) => draft.some((t) => t.type === type && t.value === value)
  function toggle(type, value) { draft = has(type, value) ? draft.filter((t) => !(t.type === type && t.value === value)) : [...draft, { type, value }] }
  async function findPeople() { people = pq.trim() ? (await api.employees(pq.trim()).catch(() => [])).slice(0, 8) : [] }
  async function saveDist(reset = false) {
    busy = true; err = ''
    const r = await api.docDistribution(code, reset ? [] : draft)
    busy = false
    if (!r.ok) { err = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    editing = false; msg = reset ? 'กลับไปใช้รายชื่อแจกจ่ายเริ่มต้น' : 'บันทึกรายชื่อแจกจ่ายแล้ว'
    dispatch('changed', r.body)
  }
  async function request() {
    busy = true; err = ''
    const r = await api.docAckRequest(code)
    busy = false
    if (!r.ok) { err = r.body.error || 'ส่งไม่สำเร็จ'; return }
    msg = `ส่งคำขอรับทราบ ${data.currentRev} แล้ว`
    dispatch('changed', r.body)
  }
</script>

{#if msg}<div class="save-ok">{msg}</div>{/if}
{#if err}<div class="merr">{err}</div>{/if}

<div class="capa-grid mb-3">
  <section class="sec">
    <div class="flex items-center gap-2 mb-2"><h2 class="!m-0">รายชื่อแจกจ่าย</h2>{#if data.isDefault}<span class="rd-cat">ค่าเริ่มต้น</span>{/if}<span class="flex-1"></span>
      {#if data.canManage && !editing}<button class="mbtn gh" on:click={startEdit}>แก้ไข</button>{/if}</div>
    {#if !editing}
      <div class="rd-ev !mt-0">{#each data.targets as t}<span class="rd-chip doc">{label(t)}</span>{:else}<span class="mut text-[12.5px]">ยังไม่กำหนด</span>{/each}</div>
      <p class="mut text-[12px] mt-2 mb-0">ผู้มีรายชื่อ {data.people.length} คน · ทุกครั้งที่ DCR อนุมัติ Rev. ใหม่ ระบบส่งคำขอรับทราบให้อัตโนมัติ (ครบกำหนด 14 วัน)</p>
    {:else}
      <label class="sgchk !mb-2"><input type="checkbox" checked={has('all', '*')} on:change={() => toggle('all', '*')} /><span>ทุกคนในบริษัท</span></label>
      <div class="text-[12px] font-bold text-mut mb-1">แผนก</div>
      <div class="ia-chips mb-2">{#each DEPTS as d}<button type="button" class="rd-chip" class:on={has('dept', d)} on:click={() => toggle('dept', d)}>{DEPTNAME[d]}</button>{/each}</div>
      <div class="text-[12px] font-bold text-mut mb-1">บทบาท</div>
      <div class="ia-chips mb-2">{#each Object.entries(ROLES) as [k, l]}<button type="button" class="rd-chip" class:on={has('role', k)} on:click={() => toggle('role', k)}>{l}</button>{/each}</div>
      <div class="sgf mr-person"><label>เพิ่มรายบุคคล</label><input bind:value={pq} on:input={findPeople} placeholder="พิมพ์ชื่อหรือรหัสพนักงาน" />
        {#if people.length}<div class="mr-people">{#each people as p}<button on:click={() => { if (!has('emp', p.empCode)) toggle('emp', p.empCode); pq = ''; people = [] }}>{p.empCode} · {p.nameTh}</button>{/each}</div>{/if}
      </div>
      <div class="ia-chips mb-2">{#each draft.filter((t) => t.type === 'emp') as t}<button type="button" class="rd-chip on" on:click={() => toggle('emp', t.value)}>{t.value} ✕</button>{/each}</div>
      <div class="sgact !justify-start">
        <button class="mbtn accent" disabled={busy || !draft.length} on:click={() => saveDist(false)}>บันทึก</button>
        <button class="mbtn gh" disabled={busy} on:click={() => saveDist(true)}>ใช้ค่าเริ่มต้น</button>
        <button class="mbtn gh" on:click={() => (editing = false)}>ยกเลิก</button>
      </div>
    {/if}
  </section>
  <section class="sec">
    <h2 class="!mb-2">ฉบับปัจจุบัน {data.currentRev || ''} <span class="mut text-[12px]">(v{data.currentVersion})</span></h2>
    <div class="rd-mv">{cur.length ? Math.round((done / cur.length) * 100) : 0}%</div>
    <div class="rd-mn"><i class="dot {cur.length && done === cur.length ? 'ok' : 'warn'}"></i>รับทราบแล้ว {done}/{cur.length} คน{#if cur.some((a) => a.overdue)} · <b class="text-danger">เลยกำหนด {cur.filter((a) => a.overdue).length}</b>{/if}</div>
    {#if data.canManage}
      <button class="mbtn mt-3 inline-flex items-center gap-1.5" disabled={busy || !notRequested.length} on:click={request}><Icon name="clipboard-check" size={14} /> ส่งคำขอรับทราบ{notRequested.length ? ` (${notRequested.length} คนที่ยังไม่ได้รับ)` : ' — ส่งครบแล้ว'}</button>
    {/if}
  </section>
</div>

{#if cur.length}
  <div class="sec !p-0 overflow-auto">
    <table class="chg-table">
      <thead><tr><th>พนักงาน</th><th>แผนก</th><th>สถานะ</th><th>ครบกำหนด</th><th>เปิดอ่าน</th><th>รับทราบ</th></tr></thead>
      <tbody>
        {#each cur as a (a.id)}
          <tr><td>{a.empName} <span class="mut">{a.empCode}</span></td><td>{DEPTNAME[a.dept] || a.dept}</td>
            <td><span class="ia-res {a.status === 'acknowledged' ? 'r-conform' : a.overdue ? 'r-major' : 'r-minor'}">{a.overdue ? 'เลยกำหนด' : STATUS[a.status]}</span></td>
            <td class="nowrap">{a.dueDate ? fmtDate(a.dueDate) : '—'}</td><td class="nowrap">{a.openedAt ? fmtWhen(a.openedAt) : '—'}</td><td class="nowrap">{a.ackedAt ? fmtWhen(a.ackedAt) : '—'}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}
  <p class="mnote">ยังไม่มีคำขอรับทราบสำหรับฉบับปัจจุบัน{data.canManage ? ' — กด “ส่งคำขอรับทราบ”' : ''}</p>
{/if}

{#if old.length}
  <button class="mbtn gh mt-3" on:click={() => (showOld = !showOld)}>{showOld ? 'ซ่อน' : 'แสดง'}ประวัติการรับทราบฉบับก่อน ({old.length})</button>
  {#if showOld}
    <table class="chg-table mt-2"><thead><tr><th>Rev.</th><th>พนักงาน</th><th>สถานะ</th><th>รับทราบ</th></tr></thead>
      <tbody>{#each old as a}<tr><td>{a.rev} (v{a.version})</td><td>{a.empName}</td><td>{STATUS[a.status]}</td><td>{a.ackedAt ? fmtWhen(a.ackedAt) : '—'}</td></tr>{/each}</tbody></table>
  {/if}
{/if}
