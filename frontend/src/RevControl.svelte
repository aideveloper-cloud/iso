<script>
  // ควบคุมฉบับ / สำเนาเอกสาร (ISO 9001:2015 ข้อ 7.5.3.2 c–d)
  //   ผู้ควบคุมเอกสาร: สำเนาฉบับเก่าที่ยังไม่เรียกคืน · เลขฉบับในไฟล์/แบบฟอร์มออนไลน์ที่ไม่ตรงทะเบียน · ทะเบียนสำเนา
  //   ทุกคน: สำเนาที่ตนถือ (ไฟล์ที่ดาวน์โหลด/สำเนาพิมพ์) และบันทึกคืน/ทำลาย
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { fmtWhen } from './lib/changeNote.js'
  import { COPY_KIND, COPY_STATUS } from './lib/copies.js'
  import CopyTable from './CopyTable.svelte'
  import Icon from './Icon.svelte'

  export let me = null
  export let refresh = 0
  const dispatch = createEventDispatcher()

  const approver = () => ['approver', 'admin'].includes(me?.role)
  const KIND = {
    rev: 'เลขฉบับในไฟล์', sheetname: 'ชื่อชีต', filename: 'ชื่อไฟล์', ref: 'อ้างถึงเอกสาร', norev: 'ไม่ระบุเลขฉบับ',
    unreadable: 'อ่านไฟล์ไม่ได้', eform: 'แบบฟอร์มออนไลน์', 'eform-draft': 'ร่างแบบฟอร์ม',
  }
  const LEVEL = { bad: ['ไม่ตรงทะเบียน', 'r-major'], warn: ['ควรแก้ไข', 'r-minor'], info: ['ข้อสังเกต', ''] }

  let view = approver() ? 'recall' : 'mine'
  let rep = null, all = [], mine = [], err = '', loading = true
  let level = 'bad', status = '', q = ''

  async function load() {
    loading = true; err = ''
    try {
      const jobs = [api.copies({ mine: '1' }).then((r) => (mine = r))]
      if (approver()) jobs.push(api.revControl().then((r) => (rep = r)), api.copies().then((r) => (all = r)))
      await Promise.all(jobs)
    } catch (e) { err = 'โหลดไม่สำเร็จ: ' + e.message }
    loading = false
  }
  onMount(load)
  $: if (refresh) load()

  $: sm = rep?.summary || {}
  $: issues = (rep?.issues || []).filter((i) => level === 'all' || i.level === level)
  $: myOpen = mine.filter((c) => ['active', 'recall'].includes(c.status))
  $: needle = q.trim().toLowerCase()
  $: reg = all.filter((c) => (!status || c.status === status)
    && (!needle || `${c.copyNo} ${c.docCode} ${c.docName} ${c.holderName} ${c.location}`.toLowerCase().includes(needle)))

  function exportCsv() {
    const cell = (v) => { let x = String(v ?? ''); if (/^[\s]*[=+\-@\t\r]/.test(x)) x = "'" + x; return '"' + x.replace(/"/g, '""') + '"' }
    const rows = [['เลขสำเนา', 'เอกสาร', 'ชื่อเอกสาร', 'ฉบับ', 'ประเภท', 'ผู้ถือ', 'รหัสผู้ถือ', 'จุดใช้งาน', 'สถานะ', 'ออกเมื่อ', 'ออกโดย', 'เหตุผลเรียกคืน', 'ปิดเมื่อ', 'ปิดโดย', 'หมายเหตุการปิด'],
      ...reg.map((c) => [c.copyNo, c.docCode, c.docName, c.rev, COPY_KIND[c.kind], c.holderName, c.holderCode, c.location, COPY_STATUS[c.status],
        fmtWhen(c.issuedAt), c.issuedName, c.recallReason, c.closedAt ? fmtWhen(c.closedAt) : '', c.closedName, c.closedNote])]
    const url = URL.createObjectURL(new Blob(['﻿' + rows.map((r) => r.map(cell).join(',')).join('\r\n')], { type: 'text/csv;charset=utf-8' }))
    const el = document.createElement('a'); el.href = url; el.download = 'document-copies.csv'; el.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="shield-check" size={20} /> ควบคุมฉบับ / สำเนาเอกสาร</h2>
  <p class="cat-desc">ISO 9001:2015 ข้อ 7.5.3.2 — ทุกสำเนาที่พิมพ์/ดาวน์โหลดถูกบันทึกและมี QR ให้สแกนตรวจว่ายังเป็นฉบับปัจจุบัน · ออกฉบับใหม่หรือยกเลิกใช้ → สำเนาฉบับเก่าต้องเรียกคืนภายใน {sm.recallDays || 14} วัน · ฉบับเก่าดาวน์โหลดไม่ได้และหน้าเอกสารประทับ SUPERSEDED/OBSOLETE</p>
</div>

{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}

{#if approver() && rep}
  <div class="rd-metrics">
    <button class="rd-metric" on:click={() => (view = 'recall')}><div class="rd-ml">ฉบับเก่าที่ยังไม่เรียกคืน<Icon name="rotate-ccw" size={18} /></div><div class="rd-mv">{sm.recall}</div><div class="rd-mn"><i class="dot {sm.recallOverdue ? 'bad' : sm.recall ? 'warn' : 'ok'}"></i>เลยกำหนด {sm.recallOverdue} รายการ</div></button>
    <button class="rd-metric" on:click={() => { view = 'check'; level = 'bad' }}><div class="rd-ml">เลขฉบับไม่ตรงทะเบียน<Icon name="alert-triangle" size={18} /></div><div class="rd-mv">{sm.bad}</div><div class="rd-mn"><i class="dot {sm.bad ? 'bad' : 'ok'}"></i>ตรวจเนื้อหาไฟล์แล้ว {sm.scanned} ฉบับ</div></button>
    <button class="rd-metric" on:click={() => { view = 'check'; level = 'warn' }}><div class="rd-ml">ควรแก้ไข<Icon name="list-checks" size={18} /></div><div class="rd-mv">{sm.warn}</div><div class="rd-mn"><i class="dot {sm.warn ? 'warn' : 'ok'}"></i>ชื่อชีต/ชื่อไฟล์/อ้างถึงเอกสารยกเลิก</div></button>
    <button class="rd-metric" on:click={() => { view = 'register'; status = 'active' }}><div class="rd-ml">สำเนาที่ใช้งาน<Icon name="qr-code" size={18} /></div><div class="rd-mv">{sm.activePrint + sm.activeFile}</div><div class="rd-mn"><i class="dot ok"></i>พิมพ์ {sm.activePrint} · ดาวน์โหลด {sm.activeFile}</div></button>
  </div>
{/if}

<div class="subtabs">
  {#if approver()}
    <button class="st-btn {view === 'recall' ? 'active' : ''}" on:click={() => (view = 'recall')}>ฉบับเก่าที่ยังใช้อยู่ <span class="st-n">{sm.recall ?? 0}</span></button>
    <button class="st-btn {view === 'check' ? 'active' : ''}" on:click={() => (view = 'check')}>ตรวจความถูกต้องของเลขฉบับ <span class="st-n">{(sm.bad ?? 0) + (sm.warn ?? 0)}</span></button>
    <button class="st-btn {view === 'register' ? 'active' : ''}" on:click={() => (view = 'register')}>ทะเบียนสำเนา</button>
  {/if}
  <button class="st-btn {view === 'mine' ? 'active' : ''}" on:click={() => (view = 'mine')}>สำเนาที่ฉันถือ <span class="st-n">{myOpen.length}</span></button>
</div>

{#if loading}
  <div class="loadwrap"><span class="spin"></span><p class="note m-0">กำลังตรวจเอกสารทั้งทะเบียน…</p></div>
{:else if view === 'recall' && rep}
  <p class="mnote">สำเนาที่พิมพ์หรือไฟล์ที่ดาวน์โหลดของฉบับที่ถูกแทนที่/ยกเลิกใช้แล้ว — ติดตามให้ผู้ถือคืนหรือทำลาย แล้วกด “บันทึกเรียกคืน” (ผู้ถือได้รับงานนี้ในกระดิ่งแจ้งเตือนด้วย)</p>
  {#if !rep.recall.length}
    <div class="empty-state"><Icon name="circle-check" size={34} stroke={1.5} /><p>ไม่มีสำเนาฉบับเก่าค้างอยู่ — ทุกจุดใช้งานใช้ฉบับปัจจุบัน</p></div>
  {:else}
    <CopyTable rows={rep.recall} {me} showCurrent on:changed={load} on:open={(e) => dispatch('open', e.detail)} />
  {/if}
{:else if view === 'check' && rep}
  <div class="flex flex-wrap items-center gap-2 mb-2">
    <div class="subtabs !mb-0">
      {#each [['bad', 'ไม่ตรงทะเบียน', sm.bad], ['warn', 'ควรแก้ไข', sm.warn], ['info', 'ข้อสังเกต', sm.info], ['all', 'ทั้งหมด', (sm.bad || 0) + (sm.warn || 0) + (sm.info || 0)]] as [k, l, n]}
        <button class="st-btn {level === k ? 'active' : ''}" on:click={() => (level = k)}>{l} <span class="st-n">{n ?? 0}</span></button>
      {/each}
    </div>
    <span class="flex-1"></span>
    <span class="mnote !m-0">อ่านเลขฉบับจากหัว/ท้ายกระดาษ เนื้อหา เซลล์ และชื่อชีต ของ {sm.scanned} ฉบับ · ตรวจไม่ได้ {sm.unscannable} ฉบับ (.doc/.pdf)</span>
  </div>
  {#if !issues.length}
    <div class="empty-state"><Icon name="circle-check" size={34} stroke={1.5} /><p>ไม่พบประเด็นในหมวดนี้</p></div>
  {:else}
    <div class="sec !p-0 overflow-auto">
      <table class="chg-table">
        <thead><tr><th>เอกสาร</th><th>ประเด็น</th><th>รายละเอียด</th><th>ระดับ</th></tr></thead>
        <tbody>
          {#each issues as i}
            <tr>
              <td><button class="code bg-transparent border-0 p-0 cursor-pointer hover:underline" on:click={() => dispatch('open', i.docCode)}>{i.docCode}</button> {i.docName}</td>
              <td class="nowrap">{KIND[i.kind] || i.kind}</td>
              <td>{i.detail}</td>
              <td class="nowrap"><span class="ia-res {LEVEL[i.level]?.[1] || ''}">{LEVEL[i.level]?.[0] || i.level}</span></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="mnote">แก้ไขเอกสารผ่านคำขอเปลี่ยนแปลง (DCR) ให้เลขฉบับในไฟล์ตรงกับฉบับที่จะอนุมัติ — ระบบไม่แก้เนื้อหาไฟล์ให้อัตโนมัติ · ผลนี้เป็นการคัดกรอง ผู้ควบคุมเอกสารต้องเปิดตรวจยืนยัน</p>
  {/if}
{:else if view === 'register'}
  <div class="flex flex-wrap items-center gap-2 mb-2">
    <select class="ia-year" bind:value={status}><option value="">ทุกสถานะ</option>{#each Object.entries(COPY_STATUS) as [k, l]}<option value={k}>{l}</option>{/each}</select>
    <input class="ia-year" placeholder="ค้นหาเลขสำเนา เอกสาร ผู้ถือ จุดใช้งาน…" bind:value={q} />
    <span class="flex-1"></span>
    <span class="mnote !m-0">{reg.length} รายการ</span>
    <button class="mbtn gh inline-flex items-center gap-1.5" on:click={exportCsv} disabled={!reg.length}><Icon name="download" size={14} /> CSV</button>
  </div>
  {#if !reg.length}
    <div class="empty-state"><Icon name="qr-code" size={34} stroke={1.5} /><p>ยังไม่มีสำเนาในทะเบียน — ออกสำเนาควบคุมได้จากหน้าเอกสาร (ปุ่ม “พิมพ์สำเนาควบคุม”)</p></div>
  {:else}
    <CopyTable rows={reg.slice(0, 1000)} {me} on:changed={load} on:open={(e) => dispatch('open', e.detail)} />
  {/if}
{:else if view === 'mine'}
  <p class="mnote">ไฟล์ต้นฉบับที่คุณดาวน์โหลดและสำเนาพิมพ์ที่คุณถือ — เมื่อเอกสารออกฉบับใหม่ ให้ลบไฟล์/คืนสำเนาเก่าแล้วกด “บันทึกเรียกคืน”</p>
  {#if !mine.length}
    <div class="empty-state"><Icon name="circle-check" size={34} stroke={1.5} /><p>คุณยังไม่มีสำเนาเอกสารที่ต้องติดตาม</p></div>
  {:else}
    <CopyTable rows={mine} {me} on:changed={load} on:open={(e) => dispatch('open', e.detail)} />
  {/if}
{/if}
