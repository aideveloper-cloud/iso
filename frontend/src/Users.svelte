<script>
  // ผู้ใช้และสิทธิ์ (ผู้ดูแลระบบ) — กำหนดบทบาท เปิด/ปิดบัญชี ตั้ง PIN ใหม่ และตารางสิทธิ์ตามบทบาท
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import { ROLES, PERMISSIONS } from './lib/mreview.js'
  import { DEPTNAME } from './lib/const.js'
  import Icon from './Icon.svelte'

  export let me = null
  let list = [], err = '', msg = '', q = '', roleF = '', showInactive = false, edit = null, saving = false, editErr = ''
  const ROLE_KEYS = Object.keys(ROLES)

  async function load() {
    err = ''
    try { list = await api.employees('', '', true) } catch (e) { err = 'โหลดรายชื่อไม่สำเร็จ: ' + e.message }
  }
  onMount(load)

  $: rows = list.filter((e) => (showInactive || e.status === 'active') && (!roleF || e.role === roleF) &&
    (!q || `${e.empCode} ${e.nameTh} ${e.nameEn} ${e.position} ${e.dept}`.toLowerCase().includes(q.toLowerCase())))
  $: counts = Object.fromEntries(ROLE_KEYS.map((r) => [r, list.filter((e) => e.status === 'active' && e.role === r).length]))

  function open(e) { edit = { ...e, pin: '' }; editErr = '' }
  function openNew() {
    edit = { isNew: true, empCode: '', nameTh: '', nameEn: '', position: '', dept: '', role: 'preparer', status: 'active', managerCode: '', pin: '' }
    editErr = ''
  }
  async function setAuditor(e, on) {
    const r = await api.iaSetAuditor(e.empCode, on)
    msg = r.ok ? `${on ? 'ขึ้นทะเบียน' : 'ถอน'} ${e.nameTh} ${on ? 'เป็น' : 'จาก'}ผู้ตรวจติดตามภายในแล้ว` : (r.body.error || 'บันทึกไม่สำเร็จ')
    load()
  }
  async function save() {
    if (edit.isNew) {
      edit.empCode = (edit.empCode || '').trim()
      if (!edit.empCode || !(edit.nameTh || '').trim()) { editErr = 'กรอกรหัสพนักงานและชื่อ-สกุล'; return }
      if (list.some((e) => e.empCode === edit.empCode)) { editErr = `รหัส ${edit.empCode} มีอยู่แล้ว — ใช้ปุ่มแก้ไขแทน`; return }
      if (!edit.pin) { editErr = 'ผู้ใช้ใหม่ต้องตั้ง PIN เริ่มต้น (ระบบจะให้เปลี่ยนเมื่อเข้าสู่ระบบครั้งแรก)'; return }
    }
    if (edit.pin && !/^\d{6}$/.test(edit.pin)) { editErr = 'PIN ใหม่ต้องเป็นตัวเลข 6 หลัก'; return }
    saving = true
    editErr = ''
    const payload = { create: !!edit.isNew, empCode: edit.empCode, nameTh: edit.nameTh, nameEn: edit.nameEn, position: edit.position, dept: edit.dept, role: edit.role, status: edit.status, managerCode: edit.managerCode }
    if (edit.pin) payload.pin = edit.pin
    const r = await api.employeeSave(payload)
    saving = false
    if (!r.ok) { editErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    msg = edit.isNew
      ? `เพิ่มผู้ใช้ ${edit.empCode} ${edit.nameTh} แล้ว — เข้าสู่ระบบด้วย PIN เริ่มต้น และต้องตั้ง PIN ใหม่ครั้งแรก`
      : `บันทึก ${edit.empCode} ${edit.nameTh} แล้ว — สิทธิ์ใหม่มีผลทันทีในคำขอถัดไป`
    edit = null
    load()
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="user" size={20} /> ผู้ใช้และสิทธิ์</h2>
  <p class="cat-desc">กำหนดบทบาทของพนักงานแต่ละคน — ระบบตรวจสิทธิ์ทุกคำขอที่ฝั่ง server และแยกหน้าที่ผู้จัดทำ/ผู้ตรวจสอบ/ผู้อนุมัติไม่ให้เป็นคนเดียวกันในรายการเดียว</p>
</div>

{#if me?.role !== 'admin'}
  <div class="merr">หน้านี้สำหรับผู้ดูแลระบบเท่านั้น</div>
{:else}
  {#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}
  {#if msg}<div class="save-ok">{msg}</div>{/if}

  <div class="subtabs">
    <button class="st-btn {!roleF ? 'active' : ''}" on:click={() => (roleF = '')}>ทั้งหมด <span class="st-n">{list.filter((e) => e.status === 'active').length}</span></button>
    {#each ROLE_KEYS as r}<button class="st-btn {roleF === r ? 'active' : ''}" on:click={() => (roleF = r)}>{ROLES[r]} <span class="st-n">{counts[r]}</span></button>{/each}
  </div>
  <div class="controls">
    <div class="search-wrap"><span class="search-ic"><Icon name="search" size={16} /></span><input placeholder="ค้นหา รหัส ชื่อ ตำแหน่ง แผนก..." bind:value={q} /></div>
    <label class="issue-toggle"><input type="checkbox" bind:checked={showInactive} /> แสดงบัญชีที่ปิดใช้งาน</label>
    <button class="mbtn accent inline-flex items-center gap-1" on:click={openNew}><Icon name="plus" size={15} /> เพิ่มผู้ใช้</button>
  </div>

  <div class="sec !p-0 overflow-auto mb-4">
    <table>
      <thead><tr><th>รหัส</th><th>ชื่อ</th><th>ตำแหน่ง</th><th>แผนก</th><th>บทบาท</th><th>ผู้ตรวจภายใน</th><th>สถานะ</th><th></th></tr></thead>
      <tbody>
        {#each rows as e (e.empCode)}
          <tr class:opacity-50={e.status !== 'active'}>
            <td class="code">{e.empCode}</td><td>{e.nameTh}</td><td>{e.position}</td><td>{DEPTNAME[e.dept] || e.dept}</td>
            <td><span class="role-pill r-{e.role}">{ROLES[e.role] || e.role}</span></td>
            <td class="text-center"><input type="checkbox" checked={e.internalAuditor} title="ขึ้นทะเบียนผู้ตรวจติดตามคุณภาพภายใน (FM-MR-011)" on:change={(ev) => setAuditor(e, ev.target.checked)} /></td>
            <td>{e.status === 'active' ? 'ใช้งาน' : 'ปิดใช้งาน'}{#if e.mustChangePin}<br /><span class="mut text-[11px]">รอตั้ง PIN ใหม่</span>{/if}</td>
            <td><button class="mbtn gh" on:click={() => open(e)}>แก้ไข</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <div class="sec overflow-auto">
    <h2 class="!mb-2">ตารางสิทธิ์ตามบทบาท</h2>
    <table class="chg-table perm-table">
      <thead><tr><th>สิทธิ์</th>{#each ROLE_KEYS as r}<th>{ROLES[r].split(' (')[0]}</th>{/each}</tr></thead>
      <tbody>
        {#each PERMISSIONS as [label, roles]}
          <tr><td>{label}</td>{#each ROLE_KEYS as r}<td class="text-center">{#if roles.includes(r)}<span class="text-ok"><Icon name="circle-check" size={15} /></span>{:else}<span class="text-faint">—</span>{/if}</td>{/each}</tr>
        {/each}
      </tbody>
    </table>
    <p class="rd-method"><Icon name="note" size={13} /> ทุกการลงนามต้องกรอก PIN ซ้ำ · ผู้ที่จัดทำรายการตรวจ/อนุมัติรายการนั้นเองไม่ได้ · ผู้ดูแลระบบได้รับยกเว้นกฎแยกหน้าที่ (ใช้เมื่อจำเป็นเท่านั้น และถูกบันทึกใน audit log) · ปิดบัญชีแล้วเซสชันใช้ไม่ได้ทันที</p>
  </div>

  {#if edit}
    <div class="sgbg" on:mousedown|self={() => (edit = null)}>
      <div class="sgbox">
        {#if edit.isNew}
          <h3>เพิ่มผู้ใช้ใหม่</h3>
          <div class="sgf"><label>รหัสพนักงาน</label><input bind:value={edit.empCode} autocomplete="off" /></div>
        {:else}
          <h3>{edit.empCode} · {edit.nameTh}</h3>
        {/if}
        <div class="sgf"><label>ชื่อ-สกุล</label><input bind:value={edit.nameTh} /></div>
        <div class="grid grid-cols-2 gap-3">
          <div class="sgf"><label>ตำแหน่ง</label><input bind:value={edit.position} /></div>
          <div class="sgf"><label>แผนก</label><select bind:value={edit.dept}><option value="">—</option>{#each Object.entries(DEPTNAME) as [k, n]}<option value={k}>{n}</option>{/each}</select></div>
        </div>
        <div class="sgf"><label>บทบาท</label><select bind:value={edit.role}>{#each ROLE_KEYS as r}<option value={r}>{ROLES[r]}</option>{/each}</select></div>
        <div class="sgf"><label>สถานะบัญชี</label><select bind:value={edit.status}><option value="active">ใช้งาน</option><option value="inactive">ปิดใช้งาน</option></select></div>
        <div class="sgf"><label>{edit.isNew ? 'PIN เริ่มต้น 6 หลัก (ผู้ใช้ต้องเปลี่ยนเมื่อเข้าครั้งแรก)' : 'ตั้ง PIN ใหม่ (เว้นว่างถ้าไม่เปลี่ยน)'}</label><input bind:value={edit.pin} type="password" maxlength="6" inputmode="numeric" autocomplete="new-password" /></div>
        {#if editErr}<div class="merr">{editErr}</div>{/if}
        <div class="sgact"><button class="mbtn gh" on:click={() => (edit = null)}>ยกเลิก</button><button class="mbtn accent" disabled={saving} on:click={save}>{saving ? 'กำลังบันทึก…' : 'บันทึก'}</button></div>
      </div>
    </div>
  {/if}
{/if}
