<script>
  // มติและงานติดตาม (9.3.3) — ร่าง: แก้ทุกช่อง · ติดตามงาน: เจ้าของงานอัปเดตความคืบหน้า/หลักฐาน, ผู้อนุมัติทวนสอบประสิทธิผล
  import { createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { ACT_STATUS, EFFECT, APPROVAL, PRIORITY, isLate } from './lib/mreview.js'
  import { fmtDate, fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let m
  export let me = null
  const dispatch = createEventDispatcher()

  let edit = null, err = '', busy = false, people = [], q = ''
  $: draftMode = m.status === 'draft' && me?.role !== 'viewer'
  $: monitoring = m.status === 'monitoring'

  function open(a) {
    edit = a
      ? JSON.parse(JSON.stringify(a))
      : { id: null, title: '', topic: '', ownerCode: '', ownerName: '', dueDate: '', priority: 'medium', reason: '', expected: '', budget: 0, resources: '', approval: 'not_required', evidence: [], comment: '', status: 'not_started', effectiveness: 'pending' }
    if (!edit.evidence.length) edit.evidence = [{ code: '', title: '', url: '' }]
    err = ''
    q = edit.ownerName || ''
    people = []
  }
  async function findPeople() {
    if (q.trim().length < 1) { people = []; return }
    try { people = (await api.employees(q.trim())).slice(0, 8) } catch (e) { people = [] }
  }
  const canUpdate = (a) => monitoring && (a.ownerCode === me?.empCode || ['approver', 'admin'].includes(me?.role))
  const canVerify = (a) => monitoring && m.can.verify && a.status === 'completed' && (a.ownerCode !== me?.empCode || me?.role === 'admin')

  async function save() {
    busy = true
    err = ''
    const ev = edit.evidence.filter((e) => e.code || e.title || e.url)
    let fields
    if (draftMode) {
      fields = { title: edit.title, topic: edit.topic, ownerCode: edit.ownerCode, dueDate: edit.dueDate, priority: edit.priority, reason: edit.reason, expected: edit.expected,
        budget: +edit.budget || 0, resources: edit.resources, comment: edit.comment, evidence: ev }
      if (m.can.approveBudget && edit.approval === 'approved') fields.approval = 'approved'
    } else {
      fields = { status: edit.status, comment: edit.comment, evidence: ev }
      if (canVerify(edit) || (m.can.verify && edit.status === 'completed')) fields.effectiveness = edit.effectiveness
    }
    const r = edit.id ? await api.mrActionUpdate(edit.id, edit.version, fields) : await api.mrActionCreate(m.id, m.version, fields)
    busy = false
    if (!r.ok) { err = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    edit = null
    dispatch('saved', r)
  }
  const topicTitle = (k) => m.topics.find((t) => t.key === k)?.title || '—'
</script>

<div class="flex items-center gap-2 mb-3">
  <b class="text-brand">งานติดตามจากมติที่ประชุม</b>
  <span class="flex-1"></span>
  {#if draftMode}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => open(null)}><Icon name="plus" size={14} /> เพิ่มงาน</button>{/if}
</div>
{#if monitoring}<p class="mnote !mt-0">รายละเอียดงานล็อกตามที่อนุมัติ — เจ้าของงานอัปเดตความคืบหน้าและหลักฐาน · ผู้อนุมัติ (ที่ไม่ใช่เจ้าของงาน) ทวนสอบประสิทธิผลก่อนปิดการประชุม</p>{/if}

{#if !m.actions.length}
  <div class="empty-state"><Icon name="flag" size={34} stroke={1.5} /><p>ยังไม่มีงานติดตาม{draftMode ? ' — กด “เพิ่มงาน”' : ''}</p></div>
{:else}
  <div class="sec !p-0 overflow-auto">
    <table class="chg-table">
      <thead><tr><th>เลขที่</th><th>งาน</th><th>หัวข้อ</th><th>ผู้รับผิดชอบ</th><th>กำหนดเสร็จ</th><th>สถานะ</th><th>ประสิทธิผล</th><th>งบ/ทรัพยากร</th><th></th></tr></thead>
      <tbody>
        {#each m.actions as a (a.id)}
          <tr>
            <td class="code">{a.refNo}</td>
            <td>{a.title}{#if a.priority === 'high'} <span class="prio p-high">สูง</span>{/if}</td>
            <td class="text-[11.5px]">{topicTitle(a.topic)}</td>
            <td>{a.ownerName || '—'}{#if a.dept}<br /><span class="mut">{a.dept}</span>{/if}</td>
            <td class="nowrap" class:text-danger={isLate(a)}>{a.dueDate ? fmtDate(a.dueDate) : '—'}{#if isLate(a)}<br /><b>เลยกำหนด</b>{/if}</td>
            <td>{ACT_STATUS[a.status]}</td>
            <td>{a.status === 'completed' ? EFFECT[a.effectiveness] : '—'}{#if a.verifiedBy}<br /><span class="mut">{a.verifiedBy}</span>{/if}</td>
            <td class="text-[11.5px]">{a.budget ? a.budget.toLocaleString('th-TH') + ' บาท' : ''}{a.resources ? ' ' + a.resources : ''}{#if a.approval !== 'not_required'}<br /><span class="mut">{APPROVAL[a.approval]}{a.approvedBy ? ' · ' + a.approvedBy : ''}</span>{/if}</td>
            <td class="nowrap">
              {#if draftMode || canUpdate(a) || canVerify(a)}<button class="mbtn gh" on:click={() => open(a)}>{draftMode ? 'แก้ไข' : canVerify(a) ? 'ทวนสอบ' : 'อัปเดต'}</button>{/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

{#if edit}
  <div class="sgbg" on:mousedown|self={() => (edit = null)}>
    <div class="sgbox wide">
      <h3>{edit.id ? edit.refNo : 'เพิ่มงานติดตาม'}</h3>
      {#if draftMode}
        <div class="sgf"><label>งานที่ต้องดำเนินการ <em>*</em></label><input bind:value={edit.title} /></div>
        <div class="grid grid-cols-2 gap-3">
          <div class="sgf"><label>จากหัวข้อ</label><select bind:value={edit.topic}><option value="">— ไม่ระบุ —</option>{#each m.topics as t}<option value={t.key}>{t.title}</option>{/each}</select></div>
          <div class="sgf"><label>ความสำคัญ</label><select bind:value={edit.priority}>{#each Object.entries(PRIORITY) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
        </div>
        <div class="sgf mr-person">
          <label>ผู้รับผิดชอบ (พนักงานในระบบ) <em>*</em></label>
          <input bind:value={q} on:input={findPeople} placeholder="พิมพ์ชื่อหรือรหัสพนักงาน" />
          {#if edit.ownerCode}<div class="mut text-[12px]">เลือก: {edit.ownerName} ({edit.ownerCode})</div>{/if}
          {#if people.length}
            <div class="mr-people">{#each people as p}<button on:click={() => { edit.ownerCode = p.empCode; edit.ownerName = p.nameTh; q = p.nameTh; people = [] }}>{p.empCode} · {p.nameTh} <span class="mut">{p.position} {p.dept}</span></button>{/each}</div>
          {/if}
        </div>
        <div class="sgf"><label>กำหนดเสร็จ <em>*</em></label><input type="date" bind:value={edit.dueDate} /></div>
        <details class="mb-3"><summary class="text-[12.5px] font-bold text-mut cursor-pointer">เหตุผล ผลที่คาดหวัง งบประมาณ และทรัพยากร</summary>
          <div class="sgf mt-2"><label>เหตุผล</label><textarea rows="2" bind:value={edit.reason}></textarea></div>
          <div class="sgf"><label>ผลที่คาดหวัง</label><textarea rows="2" bind:value={edit.expected}></textarea></div>
          <div class="grid grid-cols-2 gap-3">
            <div class="sgf"><label>งบประมาณ (บาท)</label><input type="number" min="0" bind:value={edit.budget} /></div>
            <div class="sgf"><label>ทรัพยากรที่ต้องการ</label><input bind:value={edit.resources} /></div>
          </div>
          {#if (+edit.budget > 0 || edit.resources.trim()) && m.can.approveBudget}
            <label class="sgchk"><input type="checkbox" checked={edit.approval === 'approved'} on:change={(e) => (edit.approval = e.target.checked ? 'approved' : 'pending')} /><span>อนุมัติงบ/ทรัพยากรนี้ (ผู้อนุมัติ)</span></label>
          {:else if +edit.budget > 0 || edit.resources.trim()}
            <p class="mnote">งบ/ทรัพยากรต้องให้ผู้อนุมัติอนุมัติ — แก้งบหลังอนุมัติจะกลับเป็น “รออนุมัติ”</p>
          {/if}
        </details>
      {:else}
        <div class="det-info mb-3">
          <div class="col-span-2"><b>งาน:</b> {edit.title}</div>
          <div><b>ผู้รับผิดชอบ:</b> {edit.ownerName}</div><div><b>กำหนดเสร็จ:</b> {fmtDate(edit.dueDate)}</div>
          {#if edit.expected}<div class="col-span-2"><b>ผลที่คาดหวัง:</b> {edit.expected}</div>{/if}
        </div>
        <div class="sgf"><label>สถานะ</label>
          <select bind:value={edit.status}>
            <option value="not_started">ยังไม่เริ่ม</option><option value="in_progress">กำลังดำเนินการ</option><option value="completed">เสร็จแล้ว (ต้องมีหลักฐาน)</option>
            {#if m.can.verify}<option value="cancelled">ยกเลิก (ต้องระบุเหตุผล)</option>{/if}
          </select>
        </div>
        {#if edit.status === 'completed' && m.can.verify && (edit.ownerCode !== me?.empCode || me?.role === 'admin')}
          <div class="sgf"><label>ทวนสอบประสิทธิผล</label>
            <select bind:value={edit.effectiveness}><option value="pending">รอทวนสอบ</option><option value="effective">มีประสิทธิผล</option><option value="not_effective">ไม่มีประสิทธิผล (กลับไปดำเนินการต่อ)</option></select>
          </div>
        {:else if edit.status === 'completed'}
          <p class="mnote">ประสิทธิผล: {EFFECT[edit.effectiveness]} — ผู้อนุมัติที่ไม่ใช่เจ้าของงานเป็นผู้ทวนสอบ</p>
        {/if}
      {/if}
      <div class="text-[12px] font-bold text-mut mb-1">หลักฐาน</div>
      {#each edit.evidence as e, j}
        <div class="mr-ev-row">
          <input bind:value={e.code} placeholder="รหัส/เลขที่" /><input bind:value={e.title} placeholder="ชื่อหลักฐาน" /><input bind:value={e.url} placeholder="https://…" />
          <button class="mbtn gh" on:click={() => (edit.evidence = edit.evidence.filter((_, k) => k !== j))}><Icon name="x" size={13} /></button>
        </div>
      {/each}
      <button class="mbtn gh mb-3" on:click={() => (edit.evidence = [...edit.evidence, { code: '', title: '', url: '' }])}>+ หลักฐาน</button>
      <div class="sgf"><label>ความเห็น / ความคืบหน้า</label><textarea rows="2" bind:value={edit.comment}></textarea></div>
      {#if edit.updatedAt}<p class="mut text-[11px]">อัปเดตล่าสุด {fmtWhen(edit.updatedAt)}</p>{/if}
      {#if err}<div class="merr">{err}</div>{/if}
      <div class="sgact"><button class="mbtn gh" on:click={() => (edit = null)}>ยกเลิก</button><button class="mbtn accent" disabled={busy} on:click={save}>{busy ? 'กำลังบันทึก…' : 'บันทึก'}</button></div>
    </div>
  </div>
{/if}
