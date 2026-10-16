<script>
  // บังคับเปลี่ยน PIN: บัญชีที่ใช้ PIN เริ่มต้น/PIN ที่ผู้ดูแลตั้งให้ ต้องตั้ง PIN ของตนเองก่อนใช้งาน
  import { api } from './lib/api.js'
  export let me = null
  export let onDone = () => {}
  export let onLogout = () => {}
  let oldPin = '', newPin = '', again = '', err = '', busy = false

  async function submit() {
    err = ''
    if (!/^\d{6}$/.test(newPin)) { err = 'PIN ใหม่ต้องเป็นตัวเลข 6 หลัก'; return }
    if (newPin !== again) { err = 'PIN ใหม่ทั้งสองช่องไม่ตรงกัน'; return }
    busy = true
    const { ok, body } = await api.changePin(oldPin.trim(), newPin)
    busy = false
    if (!ok) { err = body.error || 'เปลี่ยน PIN ไม่สำเร็จ'; return }
    onDone()
  }
</script>

<div class="login-wrap">
  <form class="sgbox" on:submit|preventDefault={submit}>
    <div class="logo">KG</div>
    <h3>ตั้ง PIN ใหม่ก่อนใช้งาน</h3>
    <div class="sgs">{me?.name} ({me?.empCode}) — PIN ปัจจุบันเป็น PIN เริ่มต้นหรือ PIN ที่ผู้ดูแลตั้งให้ เพื่อความปลอดภัยของลายเซ็นอิเล็กทรอนิกส์ กรุณาตั้ง PIN ที่มีเฉพาะคุณรู้</div>
    <div class="sgf"><label>PIN ปัจจุบัน</label><input bind:value={oldPin} type="password" maxlength="6" inputmode="numeric" autocomplete="current-password" /></div>
    <div class="sgf"><label>PIN ใหม่ 6 หลัก</label><input bind:value={newPin} type="password" maxlength="6" inputmode="numeric" autocomplete="new-password" /></div>
    <div class="sgf"><label>ยืนยัน PIN ใหม่</label><input bind:value={again} type="password" maxlength="6" inputmode="numeric" autocomplete="new-password" /></div>
    <p class="mnote">ห้ามใช้เลขซ้ำ (111111) เลขเรียง (เช่น 234567, 987654) หรือเลขตามรหัสพนักงาน · เปลี่ยนแล้วต้องเข้าสู่ระบบใหม่</p>
    {#if err}<div class="merr">{err}</div>{/if}
    <div class="sgact">
      <button type="button" class="mbtn gh" on:click={onLogout}>ออกจากระบบ</button>
      <button class="mbtn accent" disabled={busy}>{busy ? 'กำลังบันทึก…' : 'ตั้ง PIN ใหม่'}</button>
    </div>
  </form>
</div>

<style>
  .login-wrap { min-height: 100vh; display: flex; align-items: center; justify-content: center; background: #F5F7FA; padding: 24px; }
  form.sgbox { width: min(440px, 100%); }
  .logo { width: 46px; height: 46px; border-radius: 12px; background: #0B3679; color: #fff; display: flex; align-items: center; justify-content: center; font-weight: 700; margin-bottom: 8px; }
</style>
