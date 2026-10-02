<script>
  // ความพร้อมรับการตรวจประเมิน — คะแนนคัดกรอง + รายการตรวจจากข้อมูลจริง (อ้างอิงแนวทางจาก AI & Audit Dashboard)
  import { createEventDispatcher, onMount } from 'svelte'
  import { api } from './lib/api.js'
  import { fmtWhen } from './lib/changeNote.js'
  import { RD_STATUS, byStatus, exportCsv, exportJson } from './lib/readiness.js'
  import Icon from './Icon.svelte'

  export let r // ผลจาก /api/readiness
  export let me = null
  export let busy = false
  const dispatch = createEventDispatcher()

  let filter = 'open' // open | critical | warning | pass | all
  let runs = [], saving = false, msg = ''

  onMount(loadRuns)
  async function loadRuns() {
    try { runs = await api.readinessRuns() } catch (e) { runs = [] }
  }
  async function saveRun() {
    saving = true
    msg = ''
    const res = await api.readinessSave()
    saving = false
    if (!res.ok) { msg = res.body.error || 'บันทึกไม่สำเร็จ'; return }
    msg = `บันทึกผลครั้งนี้แล้ว (${res.body.result.score}%)`
    loadRuns()
  }

  $: canSave = ['reviewer', 'approver', 'admin'].includes(me?.role)
  $: controls = [...(r?.controls || [])].sort(byStatus)
  $: attention = controls.filter((c) => c.status !== 'pass')
  $: counts = {
    open: attention.length, critical: r.critical, warning: r.warnings, pass: r.passed, all: r.total,
  }
  $: shown = controls.filter((c) => filter === 'all' || (filter === 'open' ? c.status !== 'pass' : c.status === filter))
  // วงแหวนคะแนน: r=80 → เส้นรอบวง ≈ 502.65
  const CIRC = 2 * Math.PI * 80
  $: dash = `${(CIRC * (r?.score || 0)) / 100} ${CIRC}`
  $: prev = runs[0]

  const isDoc = (code) => !!r.docNames?.[code]
  function openEvidence(code) {
    if (isDoc(code)) dispatch('open', code)
    else if (/^(CAR|DAR|PAR)-/.test(code)) dispatch('goto', { tab: 'act' })
    else if (/^DCR-/.test(code)) dispatch('goto', { tab: 'dcr' })
    else dispatch('goto', { tab: 'forms' })
  }
</script>

<section class="rd-hero">
  <div class="rd-ring" aria-label="คะแนนความพร้อม {r.score}%">
    <svg viewBox="0 0 200 200" width="168" height="168">
      <circle cx="100" cy="100" r="80" class="ring-track" />
      <circle cx="100" cy="100" r="80" class="ring-progress" stroke-dasharray={dash} transform="rotate(-90 100 100)" />
      <text x="100" y="104" text-anchor="middle" class="ring-number">{r.score}<tspan class="ring-percent">%</tspan></text>
      <text x="100" y="128" text-anchor="middle" class="ring-label">READINESS</text>
    </svg>
  </div>
  <div class="rd-copy">
    <div class="rd-eyebrow"><Icon name="shield" size={14} /> คัดกรองความพร้อมรับการตรวจ ISO 9001:2015</div>
    <h2>
      {#if r.critical}มีประเด็นวิกฤต {r.critical} ข้อ ต้องแก้ก่อนวันตรวจ
      {:else if r.warnings}พร้อมตรวจเกือบครบ — เหลือติดตาม {r.warnings} ข้อ
      {:else}หลักฐานครบทุกรายการตรวจ{/if}
    </h2>
    <div class="rd-status">
      <span><i class="dot ok"></i>ผ่าน {r.passed}</span>
      <span><i class="dot warn"></i>ต้องติดตาม {r.warnings}</span>
      <span><i class="dot bad"></i>วิกฤต {r.critical}</span>
      <span class="mut">จาก {r.total} รายการ · {fmtWhen(r.asOf)}</span>
      {#if prev}<span class="mut">· ครั้งที่บันทึกล่าสุด {prev.score}% ({fmtWhen(prev.at)})</span>{/if}
    </div>
    <div class="rd-actions">
      <button class="mbtn inline-flex items-center gap-1.5" disabled={busy} on:click={() => dispatch('reload')}><Icon name="refresh-cw" size={14} /> {busy ? 'กำลังตรวจ…' : 'ตรวจใหม่'}</button>
      {#if canSave}<button class="mbtn accent inline-flex items-center gap-1.5" disabled={saving} on:click={saveRun}><Icon name="save" size={14} /> {saving ? 'กำลังบันทึก…' : 'บันทึกผลครั้งนี้'}</button>{/if}
      <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => exportCsv(r)}><Icon name="download" size={14} /> CSV</button>
      <button class="mbtn gh inline-flex items-center gap-1.5" on:click={() => exportJson(r)}><Icon name="download" size={14} /> JSON</button>
    </div>
    {#if msg}<div class="save-ok !mt-2 !mb-0">{msg}</div>{/if}
  </div>
</section>

<div class="subtabs">
  {#each [['open', 'ต้องดำเนินการ'], ['critical', 'วิกฤต'], ['warning', 'ต้องติดตาม'], ['pass', 'ผ่าน'], ['all', 'ทั้งหมด']] as [k, label]}
    <button class="st-btn {filter === k ? 'active' : ''}" on:click={() => (filter = k)}>{label} <span class="st-n">{counts[k]}</span></button>
  {/each}
</div>

{#if !shown.length}
  <div class="empty-state"><Icon name="circle-check" size={34} stroke={1.5} /><p>ไม่มีรายการในหมวดนี้</p></div>
{:else}
  <div class="rd-grid">
    {#each shown as c (c.id)}
      <div class="rd-item s-{c.status}">
        <div class="rd-top">
          <span class="rd-badge s-{c.status}">{RD_STATUS[c.status]}</span>
          <span class="rd-clause">ข้อ {c.clause}</span>
          <span class="rd-cat">{c.category}</span>
        </div>
        <div class="rd-title">{c.title}</div>
        <div class="rd-detail">{c.detail}</div>
        {#if c.evidence.length}
          <div class="rd-ev">
            {#each c.evidence.slice(0, 12) as code}
              <button class="rd-chip" class:doc={isDoc(code)} title={r.docNames?.[code] || code} on:click={() => openEvidence(code)}>{code}</button>
            {/each}
            {#if c.evidence.length > 12}<span class="mut text-[11px]">+{c.evidence.length - 12}</span>{/if}
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

{#if runs.length}
  <h3 class="rd-sub">ประวัติผลที่บันทึก</h3>
  <div class="sec !p-0 overflow-auto">
    <table class="chg-table">
      <thead><tr><th>วันเวลา</th><th>คะแนน</th><th>ผ่าน</th><th>ต้องติดตาม</th><th>วิกฤต</th><th>บันทึกโดย</th></tr></thead>
      <tbody>
        {#each runs as x}
          <tr><td class="nowrap">{fmtWhen(x.at)}</td><td><b>{x.score}%</b></td><td>{x.passed}</td><td>{x.warnings}</td><td>{x.critical}</td><td>{x.actorName}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<p class="rd-method"><Icon name="note" size={13} /> {r.methodology}</p>
