<script>
  // ข้อมูลนำเข้า 13 หัวข้อ (9.3.2) — ระเบียนจากระบบ (อ่านอย่างเดียว) + ระเบียนบันทึกเองพร้อมหลักฐาน
  import { createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import { HEALTH } from './lib/mreview.js'
  import { fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let m
  export let me = null
  export let topic = null
  const dispatch = createEventDispatcher()

  $: editable = !!m.can?.edit
  $: cur = m.topics.find((t) => t.key === topic) || m.topics[0]
  $: checkOf = (k) => m.completeness.checks.find((c) => c.key === k)

  // ฟอร์มของหัวข้อที่เลือก (คัดลอกจาก m เมื่อเปลี่ยนหัวข้อหรือข้อมูลถูกบันทึก)
  let draft = null, loadedKey = '', busy = false, err = ''
  $: { const k = `${cur.key}:${m.version}`; if (k !== loadedKey) { loadedKey = k; reset() } }
  function reset() {
    const i = m.inputs.find((x) => x.key === cur.key)
    draft = JSON.parse(JSON.stringify(i))
    draft.manual = draft.records.filter((r) => r.source !== 'system')
    for (const r of draft.manual) r.metric = r.metric || { direction: 'gte' }
    err = ''
  }
  $: system = (m.inputs.find((x) => x.key === cur.key)?.records || []).filter((r) => r.source === 'system')
  $: dirty = draft && JSON.stringify(strip(draft)) !== JSON.stringify(strip(withManual(m.inputs.find((x) => x.key === cur.key))))
  function withManual(i) { return { ...i, manual: i.records.filter((r) => r.source !== 'system') } }
  function strip(d) { return { discussion: d.discussion, decision: d.decision, notApplicable: d.notApplicable, reviewed: d.reviewed, manual: d.manual } }

  function addRecord() {
    draft.manual = [...draft.manual, { id: '', title: '', owner: '', status: 'good', note: '', evidence: [{ code: '', title: '', url: '' }], metric: { direction: 'gte' } }]
  }
  function removeRecord(i) { draft.manual = draft.manual.filter((_, j) => j !== i) }
  function addEvidence(r) { r.evidence = [...r.evidence, { code: '', title: '', url: '' }]; draft = draft }
  function removeEvidence(r, j) { r.evidence = r.evidence.filter((_, k) => k !== j); draft = draft }

  async function save(next = false) {
    busy = true
    err = ''
    const num = (v) => (v === '' || v == null || isNaN(+v) ? null : +v)
    const records = draft.manual.map((r) => ({ ...r, metric: { ...r.metric, actual: num(r.metric.actual), target: num(r.metric.target), previous: num(r.metric.previous) } }))
    const r = await api.mrInput(m.id, m.version, { key: cur.key, discussion: draft.discussion, decision: draft.decision, notApplicable: draft.notApplicable, reviewed: draft.reviewed, records })
    busy = false
    if (!r.ok) { err = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    dispatch('saved', r)
    if (next) {
      const idx = m.topics.findIndex((t) => t.key === cur.key)
      topic = m.topics[(idx + 1) % m.topics.length].key
    }
  }
  async function markAndNext() { draft.reviewed = true; await save(true) }
  function pick(k) {
    if (dirty && !confirm('มีข้อมูลที่ยังไม่บันทึกในหัวข้อนี้ — เปลี่ยนหัวข้อโดยไม่บันทึก?')) return
    topic = k
  }
  const evClick = (e) => { if (e.url) window.open(e.url, '_blank', 'noopener'); else if (e.code) dispatch('doc', e.code) }
</script>

<div class="cl-wrap">
  <nav class="cl-list" aria-label="หัวข้อข้อมูลนำเข้า">
    {#each m.topics as t}
      {@const c = checkOf(t.key)}
      <button class="cl-item {cur.key === t.key ? 'active' : ''}" on:click={() => pick(t.key)} title={c?.reason}>
        <span class="rd-dot {c?.pass ? 's-pass' : 's-warning'}"></span>
        <span class="cl-no !w-[58px]">{t.clause.replace('9.3.2 ', '')}</span>
        <span class="cl-t">{t.title}</span>
      </button>
    {/each}
  </nav>

  {#if draft}
    <section class="cl-detail">
      <div class="flex flex-wrap items-center gap-2 mb-1">
        <span class="rd-badge {checkOf(cur.key)?.pass ? 's-pass' : 's-warning'}">{checkOf(cur.key)?.reason}</span>
        <span class="rd-cat">ข้อ {cur.clause}</span>
      </div>
      <h3 class="cl-h">{cur.title}</h3>
      {#if draft.reviewedBy}<p class="mut text-[12px] !mt-0">ทบทวนโดย {draft.reviewedBy} · {fmtWhen(draft.reviewedAt)}</p>{/if}

      <h4 class="rd-sub">ข้อมูลจากระบบ ({system.length})</h4>
      {#if !system.length}
        <p class="mut text-[12.5px]">ระบบยังไม่มีข้อมูลหัวข้อนี้ — บันทึกเพิ่มเองพร้อมหลักฐานด้านล่าง หรือระบุเหตุผลที่ไม่เกี่ยวข้อง</p>
      {:else}
        {#each system as r}
          <div class="mr-rec">
            <div class="flex items-center gap-2"><span class="rd-badge s-{r.status === 'good' ? 'pass' : r.status === 'critical' ? 'critical' : 'warning'}">{HEALTH[r.status] || r.status}</span><b class="text-[13px]">{r.title}</b></div>
            {#if r.note}<div class="rd-detail mt-1">{r.note}</div>{/if}
            <div class="rd-ev !mt-2">{#each r.evidence as e}<button class="rd-chip doc" title={e.title} on:click={() => evClick(e)}>{e.code || e.title}</button>{/each}</div>
          </div>
        {/each}
      {/if}

      <h4 class="rd-sub">บันทึกเพิ่มเอง ({draft.manual.length})</h4>
      {#each draft.manual as r, i}
        <div class="mr-rec">
          {#if editable}
            <div class="grid gap-2 mr-rec-grid">
              <div class="sgf !mb-0"><label>เรื่อง <em>*</em></label><input bind:value={r.title} placeholder="เช่น อัตราของเสียสายการผลิตแผงรั้ว" /></div>
              <div class="sgf !mb-0"><label>สถานะ</label><select bind:value={r.status}><option value="good">เป็นไปตามแผน</option><option value="attention">ต้องติดตาม</option><option value="critical">วิกฤต</option></select></div>
              <div class="sgf !mb-0"><label>ผู้รับผิดชอบ</label><input bind:value={r.owner} /></div>
            </div>
            {#if ['objectives', 'processes', 'conformity', 'customers', 'suppliers', 'measurement'].includes(cur.key)}
              <div class="grid gap-2 mr-metric mt-2">
                <div class="sgf !mb-0"><label>ผลจริง</label><input inputmode="decimal" bind:value={r.metric.actual} /></div>
                <div class="sgf !mb-0"><label>เป้าหมาย</label><input inputmode="decimal" bind:value={r.metric.target} /></div>
                <div class="sgf !mb-0"><label>รอบก่อน</label><input inputmode="decimal" bind:value={r.metric.previous} /></div>
                <div class="sgf !mb-0"><label>หน่วย</label><input bind:value={r.metric.unit} placeholder="%" /></div>
                <div class="sgf !mb-0"><label>ทิศทาง</label><select bind:value={r.metric.direction}><option value="gte">ยิ่งมากยิ่งดี (≥)</option><option value="lte">ยิ่งน้อยยิ่งดี (≤)</option></select></div>
              </div>
            {/if}
            <div class="sgf !mt-2 !mb-0"><label>รายละเอียด</label><textarea rows="2" bind:value={r.note}></textarea></div>
            <div class="text-[12px] font-bold text-mut mt-2 mb-1">หลักฐาน (รหัสเอกสาร/เลขที่บันทึก หรือ ลิงก์) <em class="text-danger not-italic">*</em></div>
            {#each r.evidence as e, j}
              <div class="mr-ev-row">
                <input bind:value={e.code} placeholder="รหัส เช่น FM-MR-006" />
                <input bind:value={e.title} placeholder="ชื่อหลักฐาน" />
                <input bind:value={e.url} placeholder="https://… (ถ้ามี)" />
                <button class="mbtn gh" on:click={() => removeEvidence(r, j)} title="ลบหลักฐาน"><Icon name="x" size={13} /></button>
              </div>
            {/each}
            <div class="flex gap-2 mt-2">
              <button class="mbtn gh" on:click={() => addEvidence(r)}>+ หลักฐาน</button>
              <span class="flex-1"></span>
              <button class="mbtn gh text-danger" on:click={() => removeRecord(i)}>ลบระเบียนนี้</button>
            </div>
          {:else}
            <div class="flex items-center gap-2"><span class="rd-badge s-{r.status === 'good' ? 'pass' : r.status === 'critical' ? 'critical' : 'warning'}">{HEALTH[r.status]}</span><b class="text-[13px]">{r.title}</b>
              {#if r.metric?.actual != null}<span class="mut text-[12px]">{r.metric.actual}{r.metric.unit} / เป้า {r.metric.direction === 'lte' ? '≤' : '≥'} {r.metric.target}{r.metric.unit}</span>{/if}</div>
            {#if r.note}<div class="rd-detail mt-1">{r.note}</div>{/if}
            <div class="rd-ev !mt-2">{#each r.evidence as e}<button class="rd-chip doc" title={e.title} on:click={() => evClick(e)}>{e.code || e.title || e.url}</button>{/each}</div>
          {/if}
          <div class="mut text-[11px] mt-1">บันทึกโดย {r.author || me?.name}</div>
        </div>
      {/each}
      {#if editable}<button class="mbtn gh" on:click={addRecord}>+ เพิ่มระเบียน</button>{/if}

      <h4 class="rd-sub">การพิจารณาในที่ประชุม</h4>
      <div class="sgf"><label>ข้ออภิปราย</label><textarea rows="3" bind:value={draft.discussion} disabled={!editable}></textarea></div>
      <div class="sgf"><label>มติฝ่ายบริหาร (9.3.3)</label><textarea rows="3" bind:value={draft.decision} disabled={!editable} placeholder="การตัดสินใจ/โอกาสปรับปรุง/ความจำเป็นต้องเปลี่ยนแปลงระบบ/ทรัพยากรที่ต้องการ"></textarea></div>
      <div class="sgf"><label>ไม่เกี่ยวข้องในรอบนี้ (ระบุเหตุผล)</label><input bind:value={draft.notApplicable} disabled={!editable} placeholder="เว้นว่างถ้าเกี่ยวข้อง" /></div>
      {#if editable}
        <label class="sgchk"><input type="checkbox" bind:checked={draft.reviewed} /><span>ทบทวนหัวข้อนี้ครบแล้ว (ข้อมูลและหลักฐานถูกต้อง)</span></label>
        {#if err}<div class="merr">{err}</div>{/if}
        <div class="sgact !justify-start">
          <button class="mbtn" disabled={busy || !dirty} on:click={() => save(false)}>บันทึก</button>
          <button class="mbtn accent" disabled={busy} on:click={markAndNext}>ทบทวนแล้ว · ไปหัวข้อถัดไป</button>
        </div>
      {:else}
        <p class="mnote">แก้ไขได้เฉพาะสถานะร่าง</p>
      {/if}
    </section>
  {/if}
</div>
