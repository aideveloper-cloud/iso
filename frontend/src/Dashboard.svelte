<script>
  import { createEventDispatcher } from 'svelte'
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import Icon from './Icon.svelte'
  export let docs = []
  export let overview = []
  const dispatch = createEventDispatcher()

  function byType(t) {
    return docs.filter((d) => d.type === t).length
  }

  function ovItemCount(key) {
    const s = overview.find((x) => x.key === key)
    if (!s) return 0
    return (s.groups || []).reduce((a, g) => a + (g.items?.length || 0), 0)
  }

  $: genBoxes = [
    { key: 'profile', label: 'Company Profile', icon: 'building' },
    { key: 'role', label: 'บทบาทหน้าที่', icon: 'user' },
    { key: 'swot', label: 'SWOT', icon: 'layout-grid' },
    { key: 'general', label: 'ทั่วไป', icon: 'folder' },
  ].map((g) => ({ ...g, n: ovItemCount(g.key) }))

  $: procBoxes = [
    { label: 'QMR / ระเบียบปฏิบัติ (QP)', n: byType('QP'), icon: 'book-open', goto: { tab: 'proc', type: 'QP' } },
    { label: 'วิธีปฏิบัติงาน (WI)', n: byType('WI'), icon: 'wrench', goto: { tab: 'proc', type: 'WI' } },
    { label: 'แบบฟอร์ม (FM)', n: byType('FM'), icon: 'layout-grid', goto: { tab: 'reg', type: 'FM' } },
    { label: 'แผนก QC', n: docs.filter((d) => d.dept === 'QC').length, icon: 'clipboard-check', goto: { tab: 'proc', dept: 'QC' } },
  ]

  $: cards = [
    { label: 'รวมเอกสารควบคุม', value: docs.length, sub: 'เปิดทะเบียนทั้งหมด', icon: 'files', accent: true, goto: { tab: 'reg' } },
    { label: 'คู่มือคุณภาพ', value: byType('QM'), sub: 'QM', icon: 'book-open', goto: { tab: 'reg', type: 'QM' } },
    { label: 'นอกทะเบียน', value: docs.filter((d) => d.unreg).length, sub: 'รอขึ้นทะเบียน', icon: 'alert-triangle', goto: { tab: 'reg', unreg: true } },
    { label: 'ไฟล์หาย', value: docs.filter((d) => d.missing).length, sub: 'ต้องอัปโหลดใหม่', icon: 'alert-triangle', goto: { tab: 'audit' } },
  ]

  $: dc = DEPTS.map((d) => ({ d, n: docs.filter((x) => x.dept === d).length }))
  $: mx = Math.max(1, ...dc.map((x) => x.n))
  $: revised = docs.filter((d) => (d.versionCount || 1) > 1).slice(0, 8)

  function go(detail) { dispatch('goto', detail) }
  function goOv(section) { go({ tab: 'ov', section }) }
  function goDept(dept) { go({ tab: 'proc', dept }) }
</script>

<div class="cat-head">
  <h2 class="cat-title inline-flex items-center gap-2"><Icon name="dashboard" size={20} /> การแสดงไฟล์เอกสารทั้งหมด</h2>
  <p class="cat-desc">เลือกหมวดด้านล่างแล้วเข้าดูรายการ — คลิกเอกสารเพื่อเปิด ประวัติเวอร์ชัน และการอัปเดทในระบบ</p>
</div>

<div class="hub-grid">
  <button type="button" class="hub-box" on:click={() => go({ tab: 'ov' })}>
    <div class="hub-head">
      <span class="hub-ic text-brand"><Icon name="folder" size={22} /></span>
      <div>
        <b>เอกสารทั่วไป</b>
        <div class="hub-sub">ภาพรวมองค์กร · โปรไฟล์ · SWOT</div>
      </div>
      <span class="hub-go">เปิดหมวด →</span>
    </div>
    <ul class="hub-list">
      {#each genBoxes as g}
        <li>
          <button type="button" class="hub-item" on:click|stopPropagation={() => goOv(g.key)}>
            <Icon name={g.icon} size={15} />
            <span class="grow">{g.label}</span>
            <span class="hub-n">{g.n}</span>
          </button>
        </li>
      {/each}
    </ul>
  </button>

  <button type="button" class="hub-box hub-proc" on:click={() => go({ tab: 'proc' })}>
    <div class="hub-head">
      <span class="hub-ic text-accent-600"><Icon name="clipboard-list" size={22} /></span>
      <div>
        <b>วิธีการปฏิบัติงาน / ระเบียบ</b>
        <div class="hub-sub">QP · WI · FM แยกตามแผนก</div>
      </div>
      <span class="hub-go">เปิดหมวด →</span>
    </div>
    <ul class="hub-list">
      {#each procBoxes as p}
        <li>
          <button type="button" class="hub-item" on:click|stopPropagation={() => go(p.goto)}>
            <Icon name={p.icon} size={15} />
            <span class="grow">{p.label}</span>
            <span class="hub-n">{p.n}</span>
          </button>
        </li>
      {/each}
    </ul>
  </button>
</div>

<div class="hub-stage sec">
  <h2>พื้นที่แสดงเอกสาร</h2>
  <p class="mnote !mt-0 !mb-3">เลือกหมวดด้านบน หรือดูเอกสารที่มีการแก้ไขในระบบล่าสุดด้านล่าง — คลิกแถวเพื่อเปิดดูเวอร์ชันและอัปเดท</p>
  {#if revised.length}
    <table class="text-[12.5px] [&_th]:static">
      <tr><th>รหัส</th><th>ชื่อ</th><th>เวอร์ชันในระบบ</th><th></th></tr>
      {#each revised as d}
        <tr class="clickrow" on:click={() => dispatch('open', d.code)}>
          <td class="code">{d.code}</td>
          <td>{d.name}</td>
          <td><span class="verpill">v{d.latestVersion}</span> <span class="mut">({d.versionCount} เวอร์ชัน)</span></td>
          <td><span class="open inline-flex items-center gap-1"><Icon name="eye" size={13} /> เปิด / ประวัติ</span></td>
        </tr>
      {/each}
    </table>
  {:else}
    <div class="empty-state !py-8"><Icon name="files" size={34} stroke={1.5} /><p>ยังไม่มีการแก้ไขเอกสารในระบบ — เปิดจากหมวดด้านบนได้เลย</p></div>
  {/if}
</div>

<div class="cards">
  {#each cards as c}
    <button type="button" class="card clickable {c.accent ? 'accent' : ''}" on:click={() => go(c.goto)}>
      <div class="flex items-start justify-between gap-2">
        <div class="n">{c.value}</div>
        <span class="card-ic {c.accent ? 'accent text-accent-600' : 'text-brand'}"><Icon name={c.icon} size={18} /></span>
      </div>
      <div class="l">{c.label}<br /><span class="text-faint">{c.sub}</span></div>
    </button>
  {/each}
</div>

<div class="sec">
  <h2>จำนวนเอกสารแยกตามแผนก</h2>
  {#each dc as x}
    <div class="bar clickrow" role="button" tabindex="0" title="ดูเอกสารแผนกนี้" on:click={() => goDept(x.d)} on:keydown={(e) => e.key === 'Enter' && goDept(x.d)}>
      <div class="lab">{DEPTNAME[x.d]}</div><div class="track"><div class="fill" style="width:{(x.n / mx) * 100}%"></div></div><div class="v">{x.n}</div>
    </div>
  {/each}
</div>
