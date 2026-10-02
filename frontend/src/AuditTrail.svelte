<script>
  // บันทึกการใช้งาน (audit trail) — ใคร ทำอะไร เมื่อไร กับรายการใด · สำหรับผู้ตรวจสอบ/Auditor (ISO 9001 ข้อ 7.5.3)
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import { fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let me = null
  const GROUPS = {
    '': 'ทุกการกระทำ', login: 'เข้าสู่ระบบ', dcr: 'คำขอเปลี่ยนแปลงเอกสาร (DCR)', doc_: 'สถานะเอกสาร (ยกเลิกใช้/นำกลับ)',
    capa: 'CAR/DAR/PAR', mr: 'ทบทวนฝ่ายบริหาร', readiness: 'ผลความพร้อม Audit', employee: 'ผู้ใช้และสิทธิ์', overview: 'ไฟล์ภาพรวมองค์กร',
  }
  const LABEL = {
    login: 'เข้าสู่ระบบ', dcr_submit: 'ส่ง DCR', dcr_resubmit: 'ส่ง DCR ใหม่', dcr_review_pass: 'ตรวจสอบ DCR ผ่าน', dcr_review_return: 'ตีกลับ DCR (ตรวจสอบ)',
    dcr_approve_pass: 'อนุมัติ DCR', dcr_approve_return: 'ตีกลับ DCR (อนุมัติ)', dcr_cancel: 'ยกเลิก DCR', doc_obsolete: 'ยกเลิกใช้เอกสาร', doc_reinstate: 'นำเอกสารกลับมาใช้',
    capa_open: 'เปิดใบ', capa_edit: 'แก้ไขใบ', capa_start: 'เริ่มดำเนินการ', capa_submit: 'ส่งทวนสอบ', capa_verify_effective: 'ทวนสอบ: มีประสิทธิผล', capa_verify_not_effective: 'ทวนสอบ: ไม่มีประสิทธิผล', capa_cancel: 'ยกเลิกใบ',
    mr_create: 'สร้างการประชุมทบทวน', mr_submit: 'ส่งตรวจสอบ (MR)', mr_pass: 'ตรวจสอบผ่าน (MR)', mr_return: 'ตีกลับ (MR)', mr_approve: 'อนุมัติ (MR)', mr_close: 'ปิดการประชุม',
    mr_revise: 'ฉบับแก้ไข (MR)', mr_action_create: 'เพิ่มงาน (MR)', mr_action_update: 'อัปเดตงาน (MR)', mr_export: 'ส่งออก (MR)',
    readiness_run: 'บันทึกผลความพร้อม', employee_save: 'แก้ไขผู้ใช้/สิทธิ์', overview_save: 'แก้ไฟล์ภาพรวม',
  }
  const plus7 = (d) => new Date(d.getTime() + 7 * 3600e3).toISOString().slice(0, 10)
  let q = '', group = '', from = plus7(new Date(Date.now() - 30 * 86400e3)), to = plus7(new Date())
  let rows = [], loading = false, err = ''

  async function load() {
    loading = true; err = ''
    try {
      const r = await api.auditLog({ q, action: group, from, to, limit: 2000 })
      if (r.error) throw new Error(r.error)
      rows = r
    } catch (e) { err = 'โหลดไม่สำเร็จ: ' + e.message; rows = [] }
    loading = false
  }
  onMount(load)

  function exportCsv() {
    const cell = (v) => { let x = String(v ?? ''); if (/^[\s]*[=+\-@\t\r]/.test(x)) x = "'" + x; return '"' + x.replace(/"/g, '""') + '"' }
    const lines = [['วันเวลา', 'รหัสผู้ใช้', 'ชื่อ', 'การกระทำ', 'ประเภท', 'รายการ', 'รายละเอียด'], ...rows.map((r) => [fmtWhen(r.at), r.actorCode, r.actorName, LABEL[r.action] || r.action, r.entity, r.entityId, r.detail])]
    const url = URL.createObjectURL(new Blob(['﻿' + lines.map((l) => l.map(cell).join(',')).join('\r\n')], { type: 'text/csv;charset=utf-8' }))
    const a = document.createElement('a'); a.href = url; a.download = `audit-trail-${from}_${to}.csv`; a.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="list-checks" size={20} /> บันทึกการใช้งาน (Audit trail)</h2>
  <p class="cat-desc">ทุกการลงนาม อนุมัติ แก้ไข และการเปลี่ยนสิทธิ์ถูกบันทึกโดยระบบ แก้ไขหรือลบจากหน้าเว็บไม่ได้ — ใช้แสดงต่อ Auditor ได้</p>
</div>

{#if !['reviewer', 'approver', 'admin'].includes(me?.role)}
  <div class="merr">หน้านี้สำหรับผู้ตรวจสอบ ผู้อนุมัติ และผู้ดูแลระบบ</div>
{:else}
  <div class="controls">
    <div class="search-wrap"><span class="search-ic"><Icon name="search" size={16} /></span><input placeholder="ค้นหา ชื่อผู้ใช้ เลขที่ รายละเอียด..." bind:value={q} on:keydown={(e) => e.key === 'Enter' && load()} /></div>
    <select bind:value={group} on:change={load}>{#each Object.entries(GROUPS) as [k, l]}<option value={k}>{l}</option>{/each}</select>
    <input type="date" bind:value={from} on:change={load} /> <span class="mut">ถึง</span> <input type="date" bind:value={to} on:change={load} />
    <button class="mbtn" on:click={load}>ค้นหา</button>
    <button class="mbtn gh inline-flex items-center gap-1.5" disabled={!rows.length} on:click={exportCsv}><Icon name="download" size={14} /> CSV</button>
  </div>
  {#if err}<div class="merr">{err}</div>{/if}
  {#if loading}
    <div class="loadwrap"><span class="spin"></span></div>
  {:else if !rows.length}
    <div class="empty-state"><Icon name="list-checks" size={34} stroke={1.5} /><p>ไม่พบบันทึกในช่วงนี้</p></div>
  {:else}
    <div class="note">{rows.length} รายการ{rows.length >= 2000 ? ' (แสดงสูงสุด 2,000 — แคบช่วงวันที่ลง)' : ''}</div>
    <div class="sec !p-0 overflow-auto">
      <table>
        <thead><tr><th>วันเวลา</th><th>ผู้ใช้</th><th>การกระทำ</th><th>รายการ</th><th>รายละเอียด</th></tr></thead>
        <tbody>
          {#each rows as r (r.id)}
            <tr><td class="nowrap">{fmtWhen(r.at)}</td><td>{r.actorName}<br /><span class="mut">{r.actorCode}</span></td><td>{LABEL[r.action] || r.action}</td><td class="nowrap">{r.entityId}</td><td>{r.detail}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
{/if}
