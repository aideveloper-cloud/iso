<script>
  // ตรวจหลักฐานตามข้อกำหนด ISO 9001:2015 ข้อ 4–10 (แผนผังจาก data/clauses.json)
  import { createEventDispatcher } from 'svelte'
  import { RD_STATUS, byStatus } from './lib/readiness.js'
  import Icon from './Icon.svelte'

  export let r
  const dispatch = createEventDispatcher()

  const HEAD = {
    4: 'บริบทขององค์กร', 5: 'ภาวะผู้นำ', 6: 'การวางแผน', 7: 'การสนับสนุน',
    8: 'การปฏิบัติการ', 9: 'การประเมินสมรรถนะ', 10: 'การปรับปรุง',
  }
  let sel = null

  $: groups = Object.keys(HEAD).map((h) => ({
    h, title: HEAD[h], items: (r.clauses || []).filter((c) => c.clause.split('.')[0] === h),
  }))
  $: cur = (r.clauses || []).find((c) => c.clause === sel) || (r.clauses || []).find((c) => c.status !== 'pass') || r.clauses?.[0]
  // รายการตรวจระบบ (ไม่ใช่แผนผังเอกสาร) ที่อยู่ในข้อเดียวกันหรือข้อย่อย
  $: related = cur
    ? (r.controls || []).filter((c) => c.category !== 'ข้อกำหนด' && (c.clause === cur.clause || c.clause.startsWith(cur.clause + '.') || cur.clause.startsWith(c.clause + '.'))).sort(byStatus)
    : []
  const worst = (items) => items.some((c) => c.status === 'critical') ? 'critical' : items.some((c) => c.status === 'warning') ? 'warning' : 'pass'
</script>

<div class="cl-wrap">
  <nav class="cl-list" aria-label="ข้อกำหนด">
    {#each groups as g}
      <div class="cl-group"><span class="rd-dot s-{worst(g.items)}"></span> ข้อ {g.h} · {g.title}</div>
      {#each g.items as c}
        <button class="cl-item {cur?.clause === c.clause ? 'active' : ''}" on:click={() => (sel = c.clause)}>
          <span class="rd-dot s-{c.status}"></span>
          <span class="cl-no">{c.clause}</span>
          <span class="cl-t">{c.title}</span>
        </button>
      {/each}
    {/each}
  </nav>

  {#if cur}
    <section class="cl-detail">
      <div class="flex flex-wrap items-center gap-2 mb-1">
        <span class="rd-badge s-{cur.status}">{RD_STATUS[cur.status]}</span>
        {#if cur.required}<span class="rd-cat">ข้อกำหนดบังคับให้มีเอกสาร</span>{/if}
        {#if cur.excludable}<span class="rd-cat">ยกเว้นได้ตามขอบเขต</span>{/if}
      </div>
      <h3 class="cl-h">ข้อ {cur.clause} {cur.title}</h3>
      <p class="rd-detail">{cur.detail}</p>

      <h4 class="rd-sub">เอกสารหลักฐาน ({cur.docs.length})</h4>
      {#if cur.docs.length}
        <div class="overflow-auto">
          <table class="chg-table">
            <thead><tr><th>รหัส</th><th>ชื่อเอกสาร</th><th>Rev.</th><th>บังคับใช้</th><th>สถานะ</th></tr></thead>
            <tbody>
              {#each cur.docs as d}
                <tr class="clickrow" on:click={() => d.name && dispatch('open', d.code)}>
                  <td class="code">{d.code}</td><td>{d.name || '—'}</td><td class="nowrap">{d.rev || '—'}</td><td class="nowrap">{d.eff || '—'}</td>
                  <td><span class="audit-flag {d.active && d.status === 'ใช้งาน' ? 'f-ok' : d.active ? 'f-warn' : 'f-red'}">{d.status}</span></td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="mut text-[13px]">ยังไม่ได้ผูกเอกสารกับข้อนี้ — แก้ไขที่ <code>data/clauses.json</code></p>
      {/if}

      {#if related.length}
        <h4 class="rd-sub">รายการตรวจระบบที่เกี่ยวข้อง</h4>
        {#each related as c}
          <div class="cl-ctl"><span class="rd-badge s-{c.status}">{RD_STATUS[c.status]}</span> <b>{c.title}</b> <span class="mut">— {c.detail}</span></div>
        {/each}
      {/if}

      <p class="rd-method"><Icon name="note" size={13} /> หลักฐานในหน้านี้คือระเบียบปฏิบัติ/แบบฟอร์มที่ควบคุมในทะเบียน ก่อนสรุปว่าสอดคล้อง Auditor ต้องสุ่มดูบันทึกการปฏิบัติจริง ความครบถ้วน และการนำไปใช้</p>
    </section>
  {/if}
</div>

{#if r.unmapped?.length}
  <div class="sec mt-4">
    <b class="text-[13px]">เอกสารที่ยังไม่ผูกกับข้อกำหนด ({r.unmapped.length})</b>
    <div class="rd-ev mt-2">
      {#each r.unmapped as code}<button class="rd-chip doc" title={r.docNames?.[code]} on:click={() => dispatch('open', code)}>{code}</button>{/each}
    </div>
  </div>
{/if}
