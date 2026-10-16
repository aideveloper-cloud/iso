<script>
  // โปรแกรมตรวจติดตามคุณภาพภายใน (ISO 9001:2015 ข้อ 9.2) — แผนประจำปี · ความครอบคลุมข้อกำหนด · การตรวจแต่ละครั้ง
  import { onMount } from 'svelte'
  import { api } from './lib/api.js'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import { fmtDate } from './lib/changeNote.js'
  import IaDetail from './IaDetail.svelte'
  import Icon from './Icon.svelte'

  export let me = null
  export let docs = []
  export let refresh = 0

  const STATUS = { planned: 'วางแผน', in_progress: 'กำลังตรวจ', reported: 'ลงนามรายงานแล้ว', closed: 'ปิดแล้ว', cancelled: 'ยกเลิก' }
  let data = null, err = '', year = null, openId = null
  let creating = false, form = {}, formErr = '', saving = false

  async function load() {
    err = ''
    try { data = await api.iaList(year); year = data.year } catch (e) { err = 'โหลดไม่สำเร็จ: ' + e.message }
  }
  onMount(load)
  $: if (refresh) load()

  $: covered = data ? data.coverage.filter((c) => c.audits.length).length : 0
  $: years = data ? [...new Set([data.year, ...data.years, (data.year || 0) + 1])].sort((a, b) => b - a) : []
  $: topClauses = data ? [...new Set(data.coverage.map((c) => c.clause.split('.')[0]))] : []

  function newForm() {
    form = { title: '', year: data.year, clauses: [], depts: [], criteria: 'ISO 9001:2015 และระเบียบปฏิบัติของบริษัท', plannedDate: '', leadCode: '', team: [], auditeeCode: '' }
    formErr = ''
    creating = true
  }
  function toggle(list, v) { return list.includes(v) ? list.filter((x) => x !== v) : [...list, v] }
  // ผู้ตรวจที่เลือกได้: ขึ้นทะเบียนแล้ว และไม่อยู่แผนกที่ถูกตรวจ
  $: eligible = data ? data.auditors.filter((a) => !form.depts?.includes(a.dept)) : []
  async function create() {
    saving = true; formErr = ''
    const r = await api.iaCreate({ ...form, team: form.team.filter((c) => c !== form.leadCode) })
    saving = false
    if (!r.ok) { formErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    creating = false
    openId = r.body.id
    load()
  }
  let people = [], pq = ''
  async function findPeople() { people = pq.trim() ? (await api.employees(pq.trim()).catch(() => [])).slice(0, 8) : [] }
</script>

{#if openId}
  {#key openId}
    <IaDetail id={openId} {me} {docs} on:back={() => { openId = null; load() }} />
  {/key}
{:else}
  <div class="cat-head">
    <h2 class="cat-title inline-flex items-center gap-2"><Icon name="list-checks" size={20} /> ตรวจติดตามคุณภาพภายใน</h2>
    <p class="cat-desc">ISO 9001:2015 ข้อ 9.2 — วางแผนทั้งปีให้ครอบคลุมทุกข้อกำหนด · checklist จากแผนผังข้อกำหนด · ข้อบกพร่องเปิด CAR ได้ทันที · ผู้ตรวจต้องขึ้นทะเบียนและไม่ตรวจแผนกตนเอง</p>
  </div>

  {#if err}<div class="merr"><span>{err}</span><button class="retry" on:click={load}>ลองใหม่</button></div>{/if}
  {#if !data}
    {#if !err}<div class="loadwrap"><span class="spin"></span></div>{/if}
  {:else}
    <div class="flex flex-wrap items-center gap-2 mb-3">
      <span class="text-[13px] font-semibold text-mut">ปี พ.ศ.</span>
      <select class="ia-year" bind:value={year} on:change={load}>{#each years as y}<option value={y}>{y}</option>{/each}</select>
      <span class="flex-1"></span>
      {#if data.canPlan}<button class="mbtn accent inline-flex items-center gap-1.5" on:click={newForm}><Icon name="plus" size={14} /> วางแผนการตรวจ</button>{/if}
    </div>

    <div class="rd-metrics">
      <div class="rd-metric"><div class="rd-ml">การตรวจในแผนปี {data.year}<Icon name="calendar" size={18} /></div><div class="rd-mv">{data.audits.filter((a) => a.status !== 'cancelled').length}</div><div class="rd-mn"><i class="dot {data.audits.length ? 'ok' : 'bad'}"></i>เสร็จ {data.audits.filter((a) => ['reported', 'closed'].includes(a.status)).length}</div></div>
      <div class="rd-metric"><div class="rd-ml">ความครอบคลุมข้อกำหนด<Icon name="shield" size={18} /></div><div class="rd-mv">{covered}/{data.coverage.length}</div><div class="rd-mn"><i class="dot {covered === data.coverage.length ? 'ok' : 'warn'}"></i>ข้อที่อยู่ในแผน</div></div>
      <div class="rd-metric"><div class="rd-ml">ข้อบกพร่อง (NC)<Icon name="flag" size={18} /></div><div class="rd-mv">{data.audits.reduce((n, a) => n + a.summaryCounts.minor + a.summaryCounts.major, 0)}</div><div class="rd-mn"><i class="dot {data.audits.some((a) => a.openCars) ? 'warn' : 'ok'}"></i>CAR ที่ยังเปิด {data.audits.reduce((n, a) => n + a.openCars, 0)}</div></div>
      <div class="rd-metric"><div class="rd-ml">ผู้ตรวจที่ขึ้นทะเบียน<Icon name="user" size={18} /></div><div class="rd-mv">{data.auditors.length}</div><div class="rd-mn"><i class="dot {data.auditors.length >= 2 ? 'ok' : 'warn'}"></i>{data.auditors.length ? data.auditors.map((a) => a.name).slice(0, 3).join(', ') : 'กำหนดในหน้า ผู้ใช้และสิทธิ์'}</div></div>
    </div>

    {#if !data.audits.length}
      <div class="empty-state"><Icon name="calendar" size={34} stroke={1.5} /><p>ยังไม่มีแผนตรวจติดตามภายในของปี {data.year}</p></div>
    {:else}
      <div class="sec !p-0 overflow-auto mb-4">
        <table>
          <thead><tr><th>เลขที่</th><th>การตรวจ</th><th>ข้อกำหนด</th><th>แผนก</th><th>วันที่</th><th>หัวหน้าผู้ตรวจ</th><th>ผล</th><th>สถานะ</th></tr></thead>
          <tbody>
            {#each data.audits as a (a.id)}
              <tr class="clickrow" on:click={() => (openId = a.id)}>
                <td class="code nowrap">{a.refNo}</td><td>{a.title}</td>
                <td class="text-[11.5px]">{a.clauses.join(', ')}</td>
                <td class="text-[11.5px]">{a.depts.map((d) => DEPTNAME[d] || d).join(', ') || '—'}</td>
                <td class="nowrap">{a.actualDate ? fmtDate(a.actualDate) : a.plannedDate ? 'แผน ' + fmtDate(a.plannedDate) : '—'}</td>
                <td>{a.leadName}</td>
                <td class="nowrap text-[11.5px]">{#if a.summaryCounts.total}NC {a.summaryCounts.major + a.summaryCounts.minor} · OFI {a.summaryCounts.ofi}{#if a.openCars} · <b class="text-danger">CAR เปิด {a.openCars}</b>{/if}{:else}—{/if}</td>
                <td><span class="status-badge ia-{a.status}">{STATUS[a.status]}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    <div class="sec">
      <h2 class="!mb-2">ความครอบคลุมข้อกำหนดในแผนปี {data.year}</h2>
      <div class="ia-cover">
        {#each topClauses as h}
          <div class="ia-cover-col">
            <div class="cl-group">ข้อ {h}</div>
            {#each data.coverage.filter((c) => c.clause.split('.')[0] === h) as c}
              <div class="ia-cover-row" title={c.title}><span class="rd-dot {c.audits.length ? 's-pass' : 's-warning'}"></span><b>{c.clause}</b> <span class="truncate">{c.title}</span>{#if c.audits.length}<span class="mut text-[10.5px] nowrap">{c.audits.join(', ')}</span>{/if}</div>
            {/each}
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if creating}
    <div class="sgbg" on:mousedown|self={() => (creating = false)}>
      <div class="sgbox wide">
        <h3>วางแผนการตรวจติดตามภายใน</h3>
        <div class="grid grid-cols-2 gap-3">
          <div class="sgf col-span-2"><label>ชื่อการตรวจ <em>*</em></label><input bind:value={form.title} placeholder="เช่น ตรวจกระบวนการผลิตและควบคุมคุณภาพ ครั้งที่ 1/2569" /></div>
          <div class="sgf"><label>ปี พ.ศ.</label><input type="number" bind:value={form.year} /></div>
          <div class="sgf"><label>วันที่วางแผน</label><input type="date" bind:value={form.plannedDate} /></div>
        </div>
        <div class="sgf"><label>ข้อกำหนดที่ตรวจ <em>*</em></label>
          <div class="ia-chips">{#each data.coverage as c}<button type="button" class="rd-chip" class:doc={form.clauses.includes(c.clause)} class:on={form.clauses.includes(c.clause)} title={c.title} on:click={() => (form.clauses = toggle(form.clauses, c.clause))}>{c.clause}</button>{/each}</div>
        </div>
        <div class="sgf"><label>แผนกที่ถูกตรวจ</label>
          <div class="ia-chips">{#each DEPTS as d}<button type="button" class="rd-chip" class:on={form.depts.includes(d)} on:click={() => (form.depts = toggle(form.depts, d))}>{DEPTNAME[d]}</button>{/each}</div>
        </div>
        <div class="sgf"><label>หัวหน้าผู้ตรวจ <em>*</em></label>
          <select bind:value={form.leadCode}><option value="">— เลือก —</option>{#each eligible as a}<option value={a.code}>{a.name} ({DEPTNAME[a.dept] || a.dept})</option>{/each}</select>
          {#if !data.auditors.length}<p class="mnote">ยังไม่มีผู้ตรวจที่ขึ้นทะเบียน — ผู้ดูแลระบบกำหนดในหน้า “ผู้ใช้และสิทธิ์”</p>{:else if data.auditors.length > eligible.length}<p class="mnote">ซ่อนผู้ตรวจที่อยู่แผนกที่ถูกตรวจ (ข้อ 9.2.2 b)</p>{/if}
        </div>
        <div class="sgf"><label>ผู้ตรวจร่วม</label>
          <div class="ia-chips">{#each eligible.filter((a) => a.code !== form.leadCode) as a}<button type="button" class="rd-chip" class:on={form.team.includes(a.code)} on:click={() => (form.team = toggle(form.team, a.code))}>{a.name}</button>{/each}</div>
        </div>
        <div class="sgf mr-person"><label>ผู้รับการตรวจ (ผู้รับผิดชอบ CAR เริ่มต้น)</label>
          <input bind:value={pq} on:input={findPeople} placeholder="พิมพ์ชื่อหรือรหัสพนักงาน" />
          {#if form.auditeeCode}<div class="mut text-[12px]">เลือก: {form.auditeeName} ({form.auditeeCode})</div>{/if}
          {#if people.length}<div class="mr-people">{#each people as p}<button on:click={() => { form.auditeeCode = p.empCode; form.auditeeName = p.nameTh; pq = p.nameTh; people = [] }}>{p.empCode} · {p.nameTh} <span class="mut">{p.position}</span></button>{/each}</div>{/if}
        </div>
        <div class="sgf"><label>เกณฑ์การตรวจ</label><input bind:value={form.criteria} /></div>
        {#if formErr}<div class="merr">{formErr}</div>{/if}
        <div class="sgact"><button class="mbtn gh" on:click={() => (creating = false)}>ยกเลิก</button><button class="mbtn accent" disabled={saving} on:click={create}>{saving ? 'กำลังบันทึก…' : 'บันทึกแผน'}</button></div>
      </div>
    </div>
  {/if}
{/if}
