<script>
  // ตรวจสถานะเอกสารจาก QR ที่ประทับบนกระดาษ (ไม่ต้อง login) — ใช้หน้างานด้วยมือถือ/iPad
  //   ฉบับปัจจุบัน → ใช้งานได้ · ถูกแทนที่/ยกเลิกใช้/สำเนาถูกเรียกคืน → ห้ามใช้ พร้อมบอก Rev. ที่ต้องใช้แทน
  import { api } from './lib/api.js'
  import { fmtWhen } from './lib/changeNote.js'
  import { COPY_KIND } from './lib/copies.js'
  import Icon from './Icon.svelte'

  export let token = ''
  let v = null, err = ''

  api.verify(token).then((r) => { if (r.ok) v = r.body; else err = r.body.error || 'ตรวจสอบไม่สำเร็จ' }).catch((e) => (err = 'เชื่อมต่อระบบไม่ได้: ' + e.message))

  // สำเนาที่ปิดแล้ว (คืน/ทำลาย/สูญหาย) ไม่ควรมีอยู่ — ถือว่าห้ามใช้แม้ Rev. ยังเป็นปัจจุบัน
  $: closed = ['returned', 'destroyed', 'lost'].includes(v?.copy?.status)
  $: verdict = !v ? null
    : v.state === 'obsolete' ? { ok: false, title: 'เอกสารนี้ยกเลิกใช้แล้ว', sub: 'ห้ามนำไปใช้งาน — ส่งคืนผู้ควบคุมเอกสาร (DCC)' }
    : v.state === 'superseded' ? { ok: false, title: `ฉบับนี้ถูกแทนที่แล้ว — ใช้ ${v.currentRev}`, sub: `กระดาษในมือเป็น ${v.rev} · ห้ามใช้ ขอสำเนาฉบับปัจจุบันจาก DCC แล้วส่งคืนฉบับนี้` }
    : closed ? { ok: false, title: 'สำเนานี้ถูกปิดในทะเบียนแล้ว', sub: 'สำเนาที่บันทึกว่าคืน/ทำลาย/สูญหายไม่ควรอยู่หน้างาน — แจ้ง DCC' }
    : { ok: true, title: `ฉบับปัจจุบัน ${v.currentRev} — ใช้งานได้`, sub: v.copy?.kind === 'print' ? 'สำเนาควบคุมที่ลงทะเบียนไว้' : 'ตรงกับฉบับใช้งานในระบบ ณ เวลาที่ตรวจ' }
</script>

<div class="vf-wrap">
  <div class="sgbox vf-box">
    <div class="vf-head"><Icon name="qr-code" size={18} /> ตรวจสถานะเอกสาร ISO 9001</div>
    {#if err}
      <div class="vf-verdict bad"><Icon name="alert-triangle" size={28} /><div><b>ตรวจสอบไม่ได้</b><p>{err}</p></div></div>
    {:else if !v}
      <div class="loadwrap"><span class="spin"></span></div>
    {:else}
      <div class="vf-verdict {verdict.ok ? 'ok' : 'bad'}">
        <Icon name={verdict.ok ? 'circle-check' : 'alert-triangle'} size={30} />
        <div><b>{verdict.title}</b><p>{verdict.sub}</p></div>
      </div>
      <dl class="vf-dl">
        <dt>เอกสาร</dt><dd><b>{v.docCode}</b> {v.docName}</dd>
        {#if v.deptName}<dt>หน่วยงาน</dt><dd>{v.deptName}</dd>{/if}
        <dt>ฉบับบนกระดาษ</dt><dd>{v.rev || '—'}</dd>
        <dt>ฉบับปัจจุบัน</dt><dd>{v.state === 'obsolete' ? 'ยกเลิกใช้' : v.currentRev}{#if v.currentEff && v.state !== 'obsolete'} · บังคับใช้ {v.currentEff}{/if}</dd>
        {#if v.copy}
          <dt>เลขสำเนา</dt><dd>{v.copy.copyNo} · {COPY_KIND[v.copy.kind] || v.copy.kind}</dd>
          {#if v.copy.location}<dt>จุดใช้งาน</dt><dd>{v.copy.location}</dd>{/if}
          {#if v.copy.holderName}<dt>ผู้ถือ</dt><dd>{v.copy.holderName}</dd>{/if}
          <dt>ออกเมื่อ</dt><dd>{fmtWhen(v.copy.issuedAt)}</dd>
          {#if v.copy.status === 'recall'}<dt>สถานะสำเนา</dt><dd class="text-danger">ต้องเรียกคืน — {v.copy.recallReason}</dd>{/if}
        {/if}
        <dt>ตรวจเมื่อ</dt><dd>{fmtWhen(v.checkedAt)}</dd>
      </dl>
    {/if}
    <p class="vf-note">ข้อมูลจากระบบควบคุมเอกสาร บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด — หน้านี้ไม่แสดงเนื้อหาเอกสาร</p>
  </div>
</div>

<style>
  .vf-wrap { min-height: 100vh; display: flex; align-items: flex-start; justify-content: center; background: #F5F7FA; padding: 24px 16px; }
  .vf-box { max-width: 480px; }
  .vf-head { display: flex; align-items: center; gap: 6px; font-size: 13px; font-weight: 700; color: #0B3679; margin-bottom: 14px; }
  .vf-verdict { display: flex; gap: 12px; align-items: flex-start; border-radius: 10px; padding: 14px; margin-bottom: 14px; }
  .vf-verdict b { font-size: 17px; line-height: 1.35; display: block; }
  .vf-verdict p { margin: 4px 0 0; font-size: 13px; }
  .vf-verdict.ok { background: #E6F7EE; color: #1F9D63; }
  .vf-verdict.bad { background: #FBEAEA; color: #C0504D; }
  .vf-dl { display: grid; grid-template-columns: max-content 1fr; gap: 6px 12px; margin: 0; font-size: 13.5px; }
  .vf-dl dt { color: #667085; }
  .vf-dl dd { margin: 0; color: #212529; overflow-wrap: anywhere; }
  .vf-note { margin: 16px 0 0; font-size: 11.5px; color: #9AA3B2; }
</style>
