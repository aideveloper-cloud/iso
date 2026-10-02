<script>
  // บันทึกการประชุม/รายงานทบทวนฝ่ายบริหาร — พิมพ์หรือบันทึกเป็น PDF จากเบราว์เซอร์ · ส่งออก JSON ครบชุด
  import { api } from './lib/api.js'
  import { MR_STATUS, HEALTH, ACT_STATUS, EFFECT, SIG_STAGE, SIG_ACTION } from './lib/mreview.js'
  import { fmtDate, fmtWhen } from './lib/changeNote.js'
  import Icon from './Icon.svelte'

  export let m
  let err = ''

  async function exportJson() {
    err = ''
    try {
      const data = await api.mrExport(m.id)
      const url = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' }))
      const a = document.createElement('a')
      a.href = url
      a.download = `${m.refNo}${m.revision ? '-rev' + m.revision : ''}.json`
      a.click()
      setTimeout(() => URL.revokeObjectURL(url), 1000)
    } catch (e) { err = 'ส่งออกไม่สำเร็จ: ' + e.message }
  }
  // ลายเซ็นล่าสุดของแต่ละขั้นในรอบปัจจุบัน (หลังการตีกลับครั้งล่าสุด)
  $: lastReturn = m.signatures.map((x) => x.action).lastIndexOf('return')
  $: current = m.signatures.slice(lastReturn + 1)
  $: signer = (stage) => [...current].reverse().find((x) => x.stage === stage && x.action !== 'return')
  $: draftLabel = !['monitoring', 'closed', 'superseded'].includes(m.status)
</script>

<div class="no-print flex flex-wrap gap-2 mb-3">
  <button class="mbtn accent inline-flex items-center gap-1.5" on:click={() => window.print()}><Icon name="printer" size={14} /> พิมพ์ / บันทึก PDF</button>
  <button class="mbtn gh inline-flex items-center gap-1.5" on:click={exportJson}><Icon name="download" size={14} /> ส่งออก JSON ครบชุด</button>
  {#if draftLabel}<span class="mnote">ยังไม่อนุมัติ — เอกสารที่พิมพ์จะมีป้าย “ร่าง”</span>{/if}
</div>
{#if err}<div class="merr">{err}</div>{/if}

<article class="mr-minutes" class:is-draft={draftLabel}>
  <header class="mm-head">
    <div>
      <div class="mm-co">บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด</div>
      <h2>รายงานการประชุมทบทวนของฝ่ายบริหาร</h2>
      <div class="mm-sub">ISO 9001:2015 ข้อ 9.3 · {m.refNo}{m.revision ? ` ฉบับแก้ไข ${m.revision}` : ''} · สถานะ {MR_STATUS[m.status]}</div>
    </div>
    {#if draftLabel}<div class="mm-draft">ร่าง</div>{/if}
  </header>

  <table class="mm-table">
    <tbody>
      <tr><th>รอบการทบทวน</th><td>{m.period}</td><th>วันที่/เวลา</th><td>{fmtDate(m.meetDate)} {m.meetTime}</td></tr>
      <tr><th>สถานที่</th><td>{m.location || '—'}</td><th>ประธาน</th><td>{m.chair || '—'}</td></tr>
      <tr><th>เลขานุการ</th><td>{m.secretary || '—'}</td><th>ทบทวนครั้งถัดไป</th><td>{m.nextDue ? fmtDate(m.nextDue) : '—'}</td></tr>
      <tr><th>ผู้เข้าประชุม</th><td colspan="3" class="whitespace-pre-line">{m.attendees || '—'}</td></tr>
    </tbody>
  </table>

  <h3>1. ข้อมูลนำเข้า (9.3.2)</h3>
  {#each m.topics as t, i}
    {@const inp = m.inputs.find((x) => x.key === t.key)}
    <section class="mm-topic">
      <h4>1.{i + 1} {t.title} <span class="mut">({t.clause})</span></h4>
      {#if inp.notApplicable}<p><i>ไม่เกี่ยวข้อง: {inp.notApplicable}</i></p>{/if}
      {#if inp.records.length}
        <ul>
          {#each inp.records as r}
            <li><b>[{HEALTH[r.status] || r.status}]</b> {r.title}{#if r.note} — {r.note}{/if}
              <span class="mut">(หลักฐาน: {r.evidence.map((e) => e.code || e.title || e.url).join(', ') || '—'}{r.source === 'manual' ? ` · บันทึกโดย ${r.author}` : ''})</span></li>
          {/each}
        </ul>
      {:else if !inp.notApplicable}<p class="mut">ไม่มีข้อมูล</p>{/if}
      {#if inp.discussion}<p><b>ข้ออภิปราย:</b> <span class="whitespace-pre-line">{inp.discussion}</span></p>{/if}
      {#if inp.decision}<p><b>มติ:</b> <span class="whitespace-pre-line">{inp.decision}</span></p>{/if}
    </section>
  {/each}

  <h3>2. มติและงานติดตาม (9.3.3)</h3>
  {#if m.actions.length}
    <table class="mm-table mm-actions">
      <thead><tr><th>เลขที่</th><th>งาน</th><th>ผู้รับผิดชอบ</th><th>กำหนดเสร็จ</th><th>สถานะ</th><th>ประสิทธิผล</th></tr></thead>
      <tbody>
        {#each m.actions as a}
          <tr><td>{a.refNo}</td><td>{a.title}{#if a.expected}<br /><span class="mut">ผลที่คาดหวัง: {a.expected}</span>{/if}{#if a.budget}<br /><span class="mut">งบ {a.budget.toLocaleString('th-TH')} บาท</span>{/if}</td>
            <td>{a.ownerName}</td><td>{a.dueDate ? fmtDate(a.dueDate) : '—'}</td><td>{ACT_STATUS[a.status]}</td><td>{a.status === 'completed' ? EFFECT[a.effectiveness] : '—'}</td></tr>
        {/each}
      </tbody>
    </table>
  {:else}<p class="mut">ไม่มีงานติดตาม</p>{/if}

  <h3>3. การลงนาม</h3>
  <div class="mm-signs">
    {#each ['prepared', 'reviewed', 'approved'] as stage}
      {@const g = signer(stage)}
      <div class="mm-sign">
        <div class="mm-sign-line">{g ? g.name : ''}</div>
        <div>{SIG_STAGE[stage]}</div>
        <div class="mut">{g ? `${g.position} · ${fmtWhen(g.at)}` : 'ยังไม่ลงนาม'}</div>
        {#if g}<div class="mm-hash">SHA-256 {g.sha256.slice(0, 16)}</div>{/if}
      </div>
    {/each}
  </div>
  <p class="mm-foot">ลงนามอิเล็กทรอนิกส์ด้วยบัญชีพนักงานและ PIN ในระบบควบคุมเอกสาร ISO · SHA-256 คือค่าตรวจสอบเนื้อหาข้อมูลนำเข้าและงานติดตาม ณ เวลาที่ลงนาม</p>

  {#if m.signatures.length}
    <h3 class="no-print">ประวัติการลงนามทั้งหมด</h3>
    <table class="chg-table no-print">
      <thead><tr><th>ขั้น</th><th>การดำเนินการ</th><th>ผู้ลงนาม</th><th>วันเวลา</th><th>ความเห็น</th><th>SHA-256</th></tr></thead>
      <tbody>{#each m.signatures as g}<tr><td>{SIG_STAGE[g.stage] || g.stage}</td><td>{SIG_ACTION[g.action] || g.action}</td><td>{g.name}</td><td class="nowrap">{fmtWhen(g.at)}</td><td>{g.comment || '—'}</td><td class="mono">{g.sha256 ? g.sha256.slice(0, 12) : '—'}</td></tr>{/each}</tbody>
    </table>
  {/if}
</article>
