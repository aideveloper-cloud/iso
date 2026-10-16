<script>
  // เครื่องมือวัดและการสอบเทียบ (ISO 9001:2015 ข้อ 7.1.5)
  //   ทะเบียน EQ-xxx → บันทึกผลสอบเทียบ (ผ่าน = ใช้งานถึงกำหนดถัดไป) → ไม่ผ่าน = หยุดใช้ + ประเมินผลกระทบต่อผลการวัดที่ผ่านมา → CAR
  import { onMount, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import { fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'
  export let me = null
  export let focus = '' // รหัสเครื่องมือที่ต้องการเปิด (จากงานของฉัน/เตรียม Audit)
  const dispatch = createEventDispatcher()

  const STATUS = { in_use: 'ใช้งาน', out_of_service: 'หยุดใช้งาน', retired: 'ยกเลิกใช้งาน' }
  const STATUS_CLS = { in_use: 's-closed', out_of_service: 's-in_progress', retired: 's-cancelled' }
  const METHOD = { external: 'ส่งสอบเทียบภายนอก', internal: 'ทวนสอบภายใน' }
  const RESULT = { pass: 'ผ่าน', fail: 'ไม่ผ่าน' }
  const IMPACT = { none: 'ไม่กระทบผลการวัด', affected: 'กระทบผลการวัด' }
  const EVENT = { create: 'ขึ้นทะเบียน', edit: 'แก้ไข', cal_pass: 'สอบเทียบผ่าน', cal_fail: 'สอบเทียบไม่ผ่าน', impact: 'ประเมินผลกระทบ', car: 'เปิด CAR',
    suspend: 'หยุดใช้งาน', resume: 'กลับมาใช้งาน', retire: 'ยกเลิกใช้งาน', reinstate: 'นำกลับเข้าทะเบียน' }
  const today = () => new Date(Date.now() + 7 * 3600e3).toISOString().slice(0, 10)
  function addMonths(d, n) {
    if (!d || !n) return ''
    const [y, m, dd] = d.split('-').map(Number)
    const last = new Date(Date.UTC(y, m - 1 + Number(n) + 1, 0)).getUTCDate()
    return new Date(Date.UTC(y, m - 1 + Number(n), Math.min(dd, last))).toISOString().slice(0, 10)
  }
  const dueText = (e) => e.daysLeft == null ? '' : e.daysLeft < 0 ? `เลย ${-e.daysLeft} วัน` : e.daysLeft === 0 ? 'วันนี้' : `อีก ${e.daysLeft} วัน`

  let list = [], loading = true, err = '', q = ''
  let filter = 'active'
  async function load() {
    loading = true; err = ''
    try {
      const r = await api.equipmentList()
      if (r.error) throw new Error(r.error)
      list = r
      if (focus) { const x = list.find((y) => y.code === focus); if (x) openDetail(x); focus = '' }
    } catch (e) { err = 'โหลดทะเบียนไม่สำเร็จ: ' + e.message }
    loading = false
  }
  onMount(load)

  const attention = (e) => e.pendingImpact > 0 || e.affectedNoCar > 0
  const FILTERS = [
    ['active', 'ในทะเบียน', (e) => e.status !== 'retired'],
    ['due', 'ครบกำหนดใน 7 วัน / เลยกำหนด', (e) => e.status === 'in_use' && ['soon', 'overdue'].includes(e.dueState)],
    ['oos', 'หยุดใช้งาน', (e) => e.status === 'out_of_service'],
    ['impact', 'ไม่ผ่าน · ค้างดำเนินการ', attention],
    ['mine', 'ที่ฉันรับผิดชอบ', (e) => e.status !== 'retired' && e.ownerCode === me?.empCode],
    ['retired', 'ยกเลิกใช้งาน', (e) => e.status === 'retired'],
    ['all', 'ทั้งหมด', () => true],
  ]
  $: counts = Object.fromEntries(FILTERS.map(([k, , fn]) => [k, list.filter(fn).length]))
  $: needle = q.trim().toLowerCase()
  $: shown = list.filter(FILTERS.find(([k]) => k === filter)[2])
    .filter((e) => !needle || [e.code, e.name, e.assetNo, e.serialNo, e.maker, e.location, e.ownerName].some((x) => (x || '').toLowerCase().includes(needle)))
  $: act = list.filter((e) => e.status !== 'retired')

  // ---- เลือกพนักงาน ----
  let people = [], pq = ''
  async function findPeople() {
    if (!pq.trim()) { people = []; return }
    try { people = (await api.employees(pq.trim())).slice(0, 8) } catch (e) { people = [] }
  }
  function pickPerson(p) { f.ownerCode = p.empCode; f.ownerName = p.nameTh; pq = p.nameTh; people = [] }

  // ---- ขึ้นทะเบียน/แก้ไข ----
  const KEYS = ['name', 'assetNo', 'serialNo', 'maker', 'location', 'dept', 'ownerCode', 'rangeText', 'accuracy', 'acceptance', 'calMethod', 'intervalMonths']
  const blank = () => ({ name: '', assetNo: '', serialNo: '', maker: '', location: '', dept: '', ownerCode: '', ownerName: '', rangeText: '', accuracy: '', acceptance: '',
    calMethod: 'external', intervalMonths: 12, nextDue: '', initialCalDate: '', initialCertRef: '', initialProvider: '' })
  let f = null, editing = null, saving = false, formErr = ''
  function openCreate() { f = blank(); editing = null; formErr = ''; pq = ''; people = [] }
  function openEdit() { f = { ...d }; editing = d; formErr = ''; pq = d.ownerName || ''; people = [] }
  async function saveForm() {
    if (!f.name.trim()) { formErr = 'กรุณาระบุชื่อเครื่องมือ'; return }
    saving = true; formErr = ''
    const fields = Object.fromEntries(KEYS.map((k) => [k, k === 'intervalMonths' ? Number(f[k]) : f[k]]))
    let r
    if (editing) r = await api.equipmentUpdate(editing.id, editing.version, { ...fields, nextDue: f.nextDue })
    else r = await api.equipmentCreate({ ...fields, nextDue: f.initialCalDate ? f.nextDue : '', initialCalDate: f.initialCalDate, initialCertRef: f.initialCertRef, initialProvider: f.initialProvider })
    saving = false
    if (!r.ok) { formErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    f = null
    d = r.body.equipment
    msg = editing ? 'บันทึกการแก้ไขแล้ว' : `ขึ้นทะเบียน ${d.code} แล้ว`
    load()
  }

  // ---- รายละเอียด ----
  let d = null, msg = '', busy = false
  async function openDetail(x) {
    msg = ''; cal = null; imp = null
    const r = await api.equipment(x.id)
    if (r.error) { err = r.error; return }
    d = r.equipment
  }
  function done(r, text) {
    if (!r.ok) { alert(r.body.error || 'ไม่สำเร็จ'); return false }
    d = r.body.equipment; msg = text; load(); return true
  }

  // ---- บันทึกผลสอบเทียบ ----
  let cal = null, calErr = ''
  function startCal() {
    cal = { calDate: today(), method: d.calMethod, result: 'pass', certRef: '', provider: '', finding: '', nextDue: '', impactNow: false, impactResult: 'none', impact: '', impactAction: '' }
    calErr = ''; imp = null
  }
  $: calNext = cal && cal.result === 'pass' ? (cal.nextDue || addMonths(cal.calDate, d?.intervalMonths)) : ''
  async function saveCal() {
    if (!cal.certRef.trim() || !cal.provider.trim()) { calErr = 'กรุณาระบุเลขที่ใบรับรองและผู้สอบเทียบ'; return }
    if (cal.result === 'fail' && !cal.finding.trim()) { calErr = 'กรุณาบันทึกผลที่พบ (ค่าที่วัดได้/ความคลาดเคลื่อน)'; return }
    busy = true; calErr = ''
    const payload = { version: d.version, calDate: cal.calDate, method: cal.method, result: cal.result, certRef: cal.certRef, provider: cal.provider, finding: cal.finding, nextDue: cal.nextDue }
    if (cal.result === 'fail' && cal.impactNow) Object.assign(payload, { impactResult: cal.impactResult, impact: cal.impact, impactAction: cal.impactAction })
    const r = await api.equipmentCalibrate(d.id, payload)
    busy = false
    if (!r.ok) { calErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    const fail = cal.result === 'fail'
    d = r.body.equipment; cal = null
    msg = fail ? 'บันทึกผลไม่ผ่านแล้ว — เครื่องมือถูกหยุดใช้งาน' + (d.pendingImpact ? ' · กรุณาประเมินผลกระทบต่อผลการวัดที่ผ่านมา' : '') : `บันทึกผลผ่านแล้ว · ครั้งถัดไป ${d.nextDue}`
    load()
  }

  // ---- ประเมินผลกระทบ (ผลไม่ผ่าน) ----
  let imp = null, impErr = ''
  function startImpact(k) { imp = { id: k.id, calDate: k.calDate, since: k.affectedSince, impactResult: 'none', impact: '', impactAction: '' }; impErr = ''; cal = null }
  async function saveImpact() {
    if (!imp.impact.trim()) { impErr = 'กรุณาบันทึกการประเมินผลกระทบ'; return }
    if (imp.impactResult === 'affected' && !imp.impactAction.trim()) { impErr = 'กรุณาระบุการดำเนินการกับผลการวัด/ผลิตภัณฑ์ที่ได้รับผลกระทบ'; return }
    busy = true; impErr = ''
    const r = await api.equipmentImpact(d.id, imp.id, { impactResult: imp.impactResult, impact: imp.impact, impactAction: imp.impactAction })
    busy = false
    if (!r.ok) { impErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    d = r.body.equipment; imp = null; msg = 'บันทึกการประเมินผลกระทบแล้ว'; load()
  }
  async function raiseCar(k) {
    if (!confirm(`เปิด CAR จากผลสอบเทียบไม่ผ่าน ${k.calDate}? (ผู้รับผิดชอบเครื่องมือเป็นผู้รับผิดชอบใบ · ติดตามต่อที่เมนู CAR / DAR / PAR)`)) return
    busy = true
    const r = await api.equipmentCar(d.id, k.id)
    busy = false
    if (done(r, '')) msg = `เปิด ${r.body.equipment.calibrations.find((x) => x.id === k.id)?.carRef || 'CAR'} แล้ว`
  }
  async function act(action) {
    const label = { suspend: 'เหตุผลการหยุดใช้งาน (เช่น ชำรุด ตกหล่น ค่าเพี้ยน)', resume: 'หลักฐานว่ากลับมาใช้งานได้', retire: 'เหตุผลการยกเลิกใช้งาน (เช่น จำหน่ายทิ้ง/ชำรุดถาวร)', reinstate: 'เหตุผลการนำกลับเข้าทะเบียน' }[action]
    const reason = prompt(label)
    if (!reason || !reason.trim()) return
    done(await api.equipmentAct(d.id, d.version, action, reason), EVENT[action] + 'แล้ว')
  }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="gauge" size={20} /> เครื่องมือวัดและการสอบเทียบ</h2>
  <p class="cat-desc">ISO 9001:2015 ข้อ 7.1.5 — ทะเบียนเครื่องมือวัด รอบสอบเทียบ และหลักฐานใบรับรอง · ผลไม่ผ่านต้องหยุดใช้และประเมินผลกระทบต่อผลการวัดที่ผ่านมา (7.1.5.2)</p>
</div>

<div class="stat-row">
  <div class="card"><div class="n">{act.length}</div><div class="l">เครื่องมือในทะเบียน</div></div>
  <div class="card accent"><div class="n">{act.filter((e) => e.status === 'in_use').length}</div><div class="l">ใช้งานอยู่</div></div>
  <div class="card"><div class="n {act.some((e) => e.overdue) ? '!text-danger' : ''}">{act.filter((e) => e.overdue).length}</div><div class="l">ใช้งานแต่เลยกำหนด</div></div>
  <div class="card"><div class="n {act.some((e) => e.status === 'in_use' && e.dueState === 'soon') ? '!text-warn' : ''}">{act.filter((e) => e.status === 'in_use' && e.dueState === 'soon').length}</div><div class="l">ครบกำหนดใน 7 วัน</div></div>
  <div class="card"><div class="n {act.some(attention) ? '!text-danger' : ''}">{act.filter(attention).length}</div><div class="l">ผลไม่ผ่านค้างดำเนินการ</div></div>
</div>

<div class="type-tabs">
  <div class="relative flex-1 max-w-[420px]"><span class="search-ic"><Icon name="search" size={16} /></span><input class="cat-search !mb-0 pl-[38px]" placeholder="ค้นหารหัส ชื่อ เลขทะเบียน สถานที่ ผู้รับผิดชอบ…" bind:value={q} /></div>
  <span class="flex-1"></span>
  {#if me && me.role !== 'viewer'}
    <button class="mbtn accent inline-flex items-center gap-1.5" on:click={openCreate}><Icon name="plus" size={15} /> ขึ้นทะเบียนเครื่องมือ</button>
  {/if}
</div>
<div class="dept-chips">
  {#each FILTERS as [k, label]}<button class="dept-chip {filter === k ? 'active' : ''}" on:click={() => (filter = k)}>{label} <span class="dc-n">{counts[k]}</span></button>{/each}
</div>

{#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}

{#if loading}
  <div class="loadwrap"><span class="spin"></span><p class="note m-0">กำลังโหลด…</p></div>
{:else if !shown.length}
  <div class="empty-state"><Icon name="gauge" size={38} stroke={1.5} /><p>{list.length ? 'ไม่มีรายการในหมวดนี้' : 'ยังไม่มีทะเบียนเครื่องมือวัด — เริ่มจากเครื่องมือที่ใช้ตรวจรับ/ตรวจสอบผลิตภัณฑ์ (เวอร์เนียร์ ตลับเมตร เครื่องชั่ง เกจวัดความหนา…)'}</p></div>
{:else}
  <div class="overflow-x-auto">
    <table class="chg-table">
      <thead><tr><th>รหัส</th><th>เครื่องมือ</th><th>สถานที่ / แผนก</th><th>ช่วงวัด · ความแม่นยำ</th><th>ผู้รับผิดชอบ</th><th>สอบเทียบล่าสุด</th><th>ครบกำหนด</th><th>สถานะ</th></tr></thead>
      <tbody>
        {#each shown as e (e.id)}
          <tr class="cursor-pointer hover:bg-brand-light/40" on:click={() => openDetail(e)}>
            <td class="nowrap font-semibold">{e.code}</td>
            <td><div class="font-semibold">{e.name}</div><div class="mut text-[12px]">{[e.assetNo && 'ทะเบียน ' + e.assetNo, e.serialNo && 'S/N ' + e.serialNo, e.maker].filter(Boolean).join(' · ') || '—'}</div></td>
            <td>{e.location || '—'}{#if e.dept}<div class="mut text-[12px]">{DEPTNAME[e.dept] || e.dept}</div>{/if}</td>
            <td class="text-[12.5px]">{e.rangeText || '—'}{#if e.accuracy}<div class="mut">± {e.accuracy}</div>{/if}</td>
            <td>{e.ownerName || '—'}</td>
            <td class="nowrap">{#if e.lastCal}{e.lastCal} <span class="res-badge r-{e.lastResult}">{RESULT[e.lastResult]}</span>{:else}<span class="mut">ยังไม่เคย</span>{/if}</td>
            <td class="nowrap {e.overdue ? 'text-danger font-bold' : e.status === 'in_use' && e.dueState === 'soon' ? 'text-warn font-bold' : ''}">{e.nextDue || '—'}{#if e.nextDue && e.status !== 'retired'}<div class="text-[11.5px] {e.overdue ? '' : 'mut'}">{dueText(e)}</div>{/if}</td>
            <td class="nowrap">
              {#if e.overdue}<span class="status-badge s-overdue">เลยกำหนด</span>
              {:else if e.pendingImpact}<span class="status-badge s-overdue">ค้างประเมินผลกระทบ</span>
              {:else}<span class="status-badge {STATUS_CLS[e.status]}" title={e.statusReason}>{STATUS[e.status]}</span>{/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

{#if d && !f}
  <div class="mbg" on:mousedown|self={() => (d = null)}>
    <div class="mbox wide">
      <header class="mhead">
        <div class="mt"><b>{d.code}</b> {d.name}
          <div class="msub">ขึ้นทะเบียนโดย {d.createdName} · {fmtWhen(d.createdAt)}</div>
        </div>
        <button class="mx inline-flex items-center justify-center" on:click={() => (d = null)}><Icon name="x" size={16} /></button>
      </header>
      <div class="mbody">
        {#if msg}<div class="save-ok">{msg}</div>{/if}
        {#if d.overdue}<div class="merr">เลยกำหนดสอบเทียบ {-d.daysLeft} วัน แต่ยังอยู่ในสถานะใช้งาน — หยุดใช้หรือสอบเทียบทันที</div>{/if}
        {#if d.pendingImpact}<div class="merr">มีผลสอบเทียบไม่ผ่านที่ยังไม่ได้ประเมินผลกระทบต่อผลการวัดที่ผ่านมา</div>{/if}
        {#if d.gaps.length && d.status !== 'retired'}<div class="merr">ข้อมูลยังไม่ครบ: {d.gaps.join(', ')}</div>{/if}
        <div class="mr-bar">
          {#if d.can.calibrate}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={startCal}><Icon name="gauge" size={14} /> บันทึกผลสอบเทียบ</button>{/if}
          {#if d.can.edit}<button class="mbtn gh" on:click={openEdit}>แก้ไข</button>{/if}
          {#if d.can.suspend}<button class="mbtn gh" on:click={() => act('suspend')}>หยุดใช้งาน</button>{/if}
          {#if d.can.resume}<button class="mbtn gh" on:click={() => act('resume')}>กลับมาใช้งาน</button>{/if}
          {#if d.can.retire}<button class="mbtn gh" on:click={() => act('retire')}>ยกเลิกใช้งาน</button>{/if}
          {#if d.can.reinstate}<button class="mbtn gh" on:click={() => act('reinstate')}>นำกลับเข้าทะเบียน</button>{/if}
          <span class="mnote">สถานะ <span class="status-badge {STATUS_CLS[d.status]}">{STATUS[d.status]}</span>{#if d.statusReason} {d.statusReason}{/if} · ครบกำหนด <b class={d.overdue ? 'text-danger' : ''}>{d.nextDue || '—'}</b></span>
        </div>

        {#if cal}
          <section class="sec">
            <h2 class="!mb-2">บันทึกผลสอบเทียบ</h2>
            <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
              <div class="sgf"><label>วันที่สอบเทียบ <em>*</em></label><input type="date" max={today()} min={d.lastCal || undefined} bind:value={cal.calDate} /></div>
              <div class="sgf"><label>วิธี</label><select bind:value={cal.method}>{#each Object.entries(METHOD) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
              <div class="sgf"><label>ผล <em>*</em></label>
                <div class="seg">{#each Object.entries(RESULT) as [k, l]}<button class={cal.result === k ? 'on r-' + k : ''} on:click={() => (cal.result = k)}>{l}</button>{/each}</div>
              </div>
              <div class="sgf"><label>ครั้งถัดไป</label>
                {#if cal.result === 'pass'}<input type="date" bind:value={cal.nextDue} min={cal.calDate} /><div class="mut text-[11.5px]">ว่าง = {calNext} (รอบ {d.intervalMonths} เดือน)</div>
                {:else}<div class="pt-1.5 mut text-[12.5px]">หยุดใช้งานจนกว่าจะสอบเทียบผ่าน</div>{/if}
              </div>
            </div>
            <div class="grid grid-cols-2 gap-3">
              <div class="sgf"><label>เลขที่ใบรับรอง / รายงาน <em>*</em></label><input bind:value={cal.certRef} placeholder="เช่น CERT-2026-0412" /></div>
              <div class="sgf"><label>ผู้สอบเทียบ (ห้องปฏิบัติการ/ผู้ทวนสอบ) <em>*</em></label><input bind:value={cal.provider} placeholder="เช่น บริษัท ... (ISO/IEC 17025)" /></div>
            </div>
            <div class="sgf"><label>ผลที่พบ / ความคลาดเคลื่อนเทียบเกณฑ์{cal.result === 'fail' ? ' *' : ''}</label><textarea rows="2" bind:value={cal.finding} placeholder={d.acceptance ? 'เกณฑ์ยอมรับ: ' + d.acceptance : 'ค่าที่วัดได้ เทียบกับเกณฑ์ยอมรับ'}></textarea></div>
            {#if cal.result === 'fail'}
              <label class="inline-flex items-center gap-2 text-[13px] mb-2"><input type="checkbox" bind:checked={cal.impactNow} /> ประเมินผลกระทบต่อผลการวัดที่ผ่านมาพร้อมกัน (หรือบันทึกภายหลังที่ผลสอบเทียบนี้)</label>
              {#if cal.impactNow}
                <div class="sgf"><label>ผลการประเมิน</label><select bind:value={cal.impactResult}>{#each Object.entries(IMPACT) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
                <div class="sgf"><label>การประเมินผลกระทบ <em>*</em></label><textarea rows="2" bind:value={cal.impact} placeholder="ผลการวัดตั้งแต่สอบเทียบผ่านครั้งก่อน ({d.lastResult === 'pass' ? d.lastCal : 'เริ่มใช้งาน'}) ยังเชื่อถือได้หรือไม่ ใช้วัดงาน/ล็อตใดบ้าง"></textarea></div>
                {#if cal.impactResult === 'affected'}<div class="sgf"><label>การดำเนินการกับผลการวัด/ผลิตภัณฑ์ <em>*</em></label><textarea rows="2" bind:value={cal.impactAction} placeholder="เช่น ตรวจซ้ำล็อตที่วัดด้วยเครื่องนี้ แจ้งลูกค้า กักกันสินค้า"></textarea></div>{/if}
              {/if}
            {/if}
            {#if calErr}<div class="merr">{calErr}</div>{/if}
            <div class="sgact !justify-start"><button class="mbtn accent" disabled={busy} on:click={saveCal}>{busy ? 'กำลังบันทึก…' : 'บันทึกผล'}</button><button class="mbtn gh" on:click={() => (cal = null)}>ยกเลิก</button></div>
          </section>
        {/if}

        {#if imp}
          <section class="sec">
            <h2 class="!mb-2">ประเมินผลกระทบ — ผลสอบเทียบไม่ผ่าน {imp.calDate} (7.1.5.2)</h2>
            <p class="mnote">ช่วงผลการวัดที่อาจได้รับผลกระทบ: <b>{imp.since || 'เริ่มใช้งาน'} – {imp.calDate}</b></p>
            <div class="sgf"><label>ผลการประเมิน</label>
              <div class="seg">{#each Object.entries(IMPACT) as [k, l]}<button class={imp.impactResult === k ? 'on i-' + k : ''} on:click={() => (imp.impactResult = k)}>{l}</button>{/each}</div>
            </div>
            <div class="sgf"><label>การประเมิน / เหตุผล <em>*</em></label><textarea rows="3" bind:value={imp.impact} placeholder="เครื่องนี้ใช้วัดอะไร ความคลาดเคลื่อนทำให้ผลตัดสินผิดหรือไม่ ตรวจซ้ำด้วยเครื่องสำรองแล้วผลเป็นอย่างไร"></textarea></div>
            {#if imp.impactResult === 'affected'}<div class="sgf"><label>การดำเนินการกับผลการวัด/ผลิตภัณฑ์ที่ได้รับผลกระทบ <em>*</em></label><textarea rows="2" bind:value={imp.impactAction}></textarea></div>{/if}
            {#if impErr}<div class="merr">{impErr}</div>{/if}
            <div class="sgact !justify-start"><button class="mbtn accent" disabled={busy} on:click={saveImpact}>{busy ? 'กำลังบันทึก…' : 'บันทึกการประเมิน'}</button><button class="mbtn gh" on:click={() => (imp = null)}>ยกเลิก</button></div>
          </section>
        {/if}

        <div class="capa-grid">
          <section class="sec">
            <h2 class="!mb-2">ข้อมูลเครื่องมือ</h2>
            <div class="det-info">
              <div><b>เลขทะเบียนเดิม:</b> {d.assetNo || '—'}</div><div><b>S/N:</b> {d.serialNo || '—'}</div>
              <div><b>ยี่ห้อ/รุ่น:</b> {d.maker || '—'}</div><div><b>ผู้รับผิดชอบ:</b> {d.ownerName || '—'}</div>
              <div><b>สถานที่:</b> {d.location || '—'}</div><div><b>แผนก:</b> {d.dept ? DEPTNAME[d.dept] || d.dept : '—'}</div>
            </div>
          </section>
          <section class="sec">
            <h2 class="!mb-2">ข้อกำหนดการสอบเทียบ</h2>
            <div class="det-info">
              <div><b>ช่วงวัด:</b> {d.rangeText || '—'}</div><div><b>ความแม่นยำ:</b> {d.accuracy || '—'}</div>
              <div class="col-span-2"><b>เกณฑ์ยอมรับ:</b> {d.acceptance || '—'}</div>
              <div><b>วิธี:</b> {METHOD[d.calMethod]}</div><div><b>รอบ:</b> ทุก {d.intervalMonths} เดือน</div>
            </div>
          </section>
        </div>

        <h3 class="rd-sub">ผลการสอบเทียบ</h3>
        {#if d.calibrations.length}
          <div class="overflow-x-auto">
            <table class="chg-table">
              <thead><tr><th>วันที่</th><th>ผล</th><th>ใบรับรอง · ผู้สอบเทียบ</th><th>ผลที่พบ</th><th>ผลกระทบ / CAR</th><th>บันทึกโดย</th></tr></thead>
              <tbody>
                {#each d.calibrations as k (k.id)}
                  <tr>
                    <td class="nowrap">{k.calDate}<div class="mut text-[11.5px]">{METHOD[k.method]}</div></td>
                    <td class="nowrap"><span class="res-badge r-{k.result}">{RESULT[k.result]}</span>{#if k.nextDue}<div class="mut text-[11.5px]">ถัดไป {k.nextDue}</div>{/if}</td>
                    <td>{k.certRef || '—'}<div class="mut text-[12px]">{k.provider}</div></td>
                    <td class="text-[12.5px] whitespace-pre-wrap">{k.finding || '—'}</td>
                    <td class="text-[12.5px]">
                      {#if k.result === 'fail'}
                        {#if k.impactResult}
                          <span class="status-badge {k.impactResult === 'affected' ? 's-overdue' : 's-closed'}">{IMPACT[k.impactResult]}</span>
                          <div class="mt-1 whitespace-pre-wrap">{k.impact}</div>
                          {#if k.impactAction}<div class="mut whitespace-pre-wrap">การดำเนินการ: {k.impactAction}</div>{/if}
                          <div class="mut text-[11.5px]">ช่วง {k.affectedSince || 'เริ่มใช้งาน'} – {k.calDate} · {k.impactBy}</div>
                        {:else}<span class="status-badge s-overdue">ยังไม่ประเมินผลกระทบ</span>{/if}
                        <div class="flex gap-1.5 mt-1.5 flex-wrap">
                          {#if k.can?.impact}<button class="mbtn accent !py-0.5" on:click={() => startImpact(k)}>ประเมินผลกระทบ</button>{/if}
                          {#if k.carRef}<button class="mbtn gh !py-0.5" on:click={() => dispatch('goto', { tab: 'act' })}>{k.carRef}</button>
                          {:else if k.can?.car}<button class="mbtn gh !py-0.5" disabled={busy} on:click={() => raiseCar(k)}>เปิด CAR</button>{/if}
                        </div>
                      {:else}—{/if}
                    </td>
                    <td class="text-[12px]">{k.recordedName}<div class="mut">{fmtWhen(k.recordedAt)}</div></td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}<p class="mnote">ยังไม่มีผลสอบเทียบ — ต้องสอบเทียบก่อนนำไปใช้งาน</p>{/if}

        {#if d.history?.length}
          <h3 class="rd-sub">ประวัติ</h3>
          <table class="chg-table"><thead><tr><th>เมื่อ</th><th>ผู้ดำเนินการ</th><th>การดำเนินการ</th><th>รายละเอียด</th></tr></thead>
            <tbody>{#each [...d.history].reverse() as h}<tr><td class="nowrap">{fmtWhen(h.at)}</td><td>{h.actorName}</td><td>{EVENT[h.event] || h.event}</td><td>{h.note || '—'}</td></tr>{/each}</tbody></table>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if f}
  <div class="sgbg" on:mousedown|self={() => (f = null)}>
    <div class="sgbox wide">
      <h3>{editing ? `แก้ไข ${editing.code}` : 'ขึ้นทะเบียนเครื่องมือวัด'}</h3>
      <div class="sgf"><label>ชื่อเครื่องมือ <em>*</em></label><input bind:value={f.name} placeholder="เช่น เวอร์เนียร์คาลิปเปอร์ ดิจิทัล 150 มม." /></div>
      <div class="grid grid-cols-2 md:grid-cols-3 gap-3">
        <div class="sgf"><label>เลขทะเบียนเดิม / ป้าย</label><input bind:value={f.assetNo} /></div>
        <div class="sgf"><label>Serial No.</label><input bind:value={f.serialNo} /></div>
        <div class="sgf"><label>ยี่ห้อ / รุ่น</label><input bind:value={f.maker} /></div>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>สถานที่ใช้งาน</label><input bind:value={f.location} placeholder="เช่น ห้อง QC, ไลน์ผลิต 2" /></div>
        <div class="sgf"><label>แผนก</label><select bind:value={f.dept}><option value="">—</option>{#each DEPTS as dp}<option value={dp}>{DEPTNAME[dp]}</option>{/each}</select></div>
      </div>
      <div class="sgf mr-person"><label>ผู้รับผิดชอบ <em>*</em></label>
        <input bind:value={pq} on:input={findPeople} placeholder="พิมพ์ชื่อหรือรหัสพนักงาน" />
        {#if f.ownerCode}<div class="mut text-[12px]">เลือก: {f.ownerName} ({f.ownerCode})</div>{/if}
        {#if people.length}<div class="mr-people">{#each people as p}<button on:click={() => pickPerson(p)}>{p.empCode} · {p.nameTh} <span class="mut">{p.position}</span></button>{/each}</div>{/if}
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="sgf"><label>ช่วงวัด <em>*</em></label><input bind:value={f.rangeText} placeholder="เช่น 0–150 มม." /></div>
        <div class="sgf"><label>ความแม่นยำ / ความละเอียด</label><input bind:value={f.accuracy} placeholder="เช่น 0.02 มม." /></div>
      </div>
      <div class="sgf"><label>เกณฑ์ยอมรับผลสอบเทียบ</label><input bind:value={f.acceptance} placeholder="เช่น ความคลาดเคลื่อนไม่เกิน ±0.03 มม. ทุกจุด" /></div>
      <div class="grid grid-cols-2 md:grid-cols-3 gap-3">
        <div class="sgf"><label>วิธีสอบเทียบ</label><select bind:value={f.calMethod}>{#each Object.entries(METHOD) as [k, l]}<option value={k}>{l}</option>{/each}</select></div>
        <div class="sgf"><label>รอบสอบเทียบ (เดือน) <em>*</em></label><input type="number" min="1" max="60" bind:value={f.intervalMonths} /></div>
        {#if editing}<div class="sgf"><label>ครบกำหนดครั้งถัดไป</label><input type="date" bind:value={f.nextDue} /></div>{/if}
      </div>
      {#if !editing}
        <section class="sec !mb-2">
          <h2 class="!mb-1 !text-[13.5px]">ผลสอบเทียบล่าสุดที่มีอยู่แล้ว (ถ้ามี)</h2>
          <p class="mnote !mt-0">ไม่ระบุ = ยังไม่เคยสอบเทียบ ระบบจะตั้งสถานะ "หยุดใช้งาน" จนกว่าจะบันทึกผลผ่าน</p>
          <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
            <div class="sgf"><label>วันที่สอบเทียบ</label><input type="date" max={today()} bind:value={f.initialCalDate} /></div>
            <div class="sgf"><label>เลขที่ใบรับรอง</label><input bind:value={f.initialCertRef} disabled={!f.initialCalDate} /></div>
            <div class="sgf"><label>ผู้สอบเทียบ</label><input bind:value={f.initialProvider} disabled={!f.initialCalDate} /></div>
            <div class="sgf"><label>ครบกำหนดครั้งถัดไป</label><input type="date" bind:value={f.nextDue} disabled={!f.initialCalDate} />{#if f.initialCalDate && !f.nextDue}<div class="mut text-[11.5px]">ว่าง = {addMonths(f.initialCalDate, f.intervalMonths)}</div>{/if}</div>
          </div>
        </section>
      {/if}
      {#if formErr}<div class="merr">{formErr}</div>{/if}
      <div class="sgact"><button class="mbtn gh" on:click={() => (f = null)}>ยกเลิก</button><button class="mbtn accent" disabled={saving} on:click={saveForm}>{saving ? 'กำลังบันทึก…' : 'บันทึก'}</button></div>
    </div>
  </div>
{/if}
