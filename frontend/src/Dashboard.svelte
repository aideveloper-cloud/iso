<script>
  import { DEPTS, DEPTNAME } from './lib/const.js'
  import Icon from './Icon.svelte'
  export let docs = []

  $: by = (t) => docs.filter((d) => d.type === t).length
  // [label, value, sub, icon(lucide), accent?]
  $: cards = [
    ['รวมเอกสาร', docs.length, 'ฉบับ (ตัวล่าสุด)', 'files', true],
    ['คู่มือคุณภาพ', by('QM'), 'QM', 'book-open', false],
    ['ระเบียบปฏิบัติ', by('QP'), 'QP', 'clipboard-list', false],
    ['วิธีปฏิบัติงาน', by('WI'), 'WI', 'wrench', false],
    ['แบบฟอร์ม', by('FM'), 'FM', 'layout-grid', false],
    ['นอกทะเบียน', docs.filter((d) => d.unreg).length, 'รอขึ้นทะเบียน', 'alert-triangle', false],
  ]
  $: dc = DEPTS.map((d) => ({ d, n: docs.filter((x) => x.dept === d).length }))
  $: mx = Math.max(1, ...dc.map((x) => x.n))
  $: dups = docs.filter((d) => d.dup > 1).slice().sort((a, b) => b.dup - a.dup)
</script>

<div class="cards">
  {#each cards as c}
    <div class="card {c[4] ? 'accent' : ''}">
      <div class="flex items-start justify-between gap-2">
        <div class="n">{c[1]}</div>
        <span class="card-ic {c[4] ? 'accent text-accent-600' : 'text-brand'}"><Icon name={c[3]} size={18} /></span>
      </div>
      <div class="l">{c[0]}<br /><span class="text-faint">{c[2]}</span></div>
    </div>
  {/each}
</div>

<div class="sec">
  <h2>จำนวนเอกสารแยกตามแผนก</h2>
  {#each dc as x}
    <div class="bar"><div class="lab">{DEPTNAME[x.d]}</div><div class="track"><div class="fill" style="width:{(x.n / mx) * 100}%"></div></div><div class="v">{x.n}</div></div>
  {/each}
</div>

<div class="sec">
  <h2>สถานะระบบ</h2>
  <div class="status-item"><span class="dot bg-ok"></span> การตรวจติดตามภายใน (IQA) รอบปี 2568 <span class="flex-1"></span><span class="badge b-ok">เสร็จแล้ว</span></div>
  <div class="status-item"><span class="dot bg-warn"></span> การประชุมทบทวนฝ่ายบริหาร (Management Review) รอบปี 2569 <span class="flex-1"></span><span class="badge b-warn">รอประชุม</span></div>
  <div class="status-item"><span class="dot bg-danger"></span> ข้อบกพร่อง/โอกาสพัฒนา (NC/OFI) ค้างดำเนินการ <span class="flex-1"></span><span class="badge b-red">5 ข้อกำหนด</span></div>
  <div class="note">ข้อกำหนดที่ยังค้าง: 5.3 (บทบาทหน้าที่), 7.1.3 (โครงสร้างพื้นฐาน), 8.5.4 (การถนอมรักษา), 9.2 (Internal Audit), 9.3 (Management Review)</div>
</div>

<div class="sec">
  <h2>การจัดการไฟล์ซ้ำ</h2>
  <div class="text-[13.5px] mb-2">ในแฟ้มต้นฉบับมีไฟล์ซ้ำหลายเวอร์ชัน <b>{dups.length}</b> รหัส — เว็บแอปนี้ชี้เฉพาะ <b>ตัวล่าสุด</b> ของแต่ละรหัสแล้ว สูงสุดได้แก่:</div>
  <table class="text-[12.5px] [&_th]:static">
    <tr><th>รหัส</th><th>ชื่อ</th><th>จำนวนไฟล์เดิม</th></tr>
    {#each dups.slice(0, 8) as d}
      <tr><td class="code">{d.code}</td><td>{d.name}</td><td><span class="dupwarn">{d.dup} ไฟล์</span></td></tr>
    {/each}
  </table>
</div>
