<script>
  import { api } from './lib/api.js'
  export let onOk = () => {}
  let empCode = ''
  let pin = ''
  let err = ''
  let busy = false

  async function submit() {
    err = ''
    busy = true
    const { ok, body } = await api.login(empCode.trim(), pin.trim())
    busy = false
    if (!ok) { err = body.error || 'เข้าสู่ระบบไม่ได้'; return }
    localStorage.setItem('isoToken', body.token)
    onOk(body.user)
  }
</script>

<div class="login-wrap">
  <form class="sgbox" on:submit|preventDefault={submit}>
    <div class="logo">KG</div>
    <h3>เข้าสู่ระบบ</h3>
    <div class="sgs">ระบบควบคุมเอกสาร ISO 9001:2015 · บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด</div>
    <div class="sgf"><label>รหัสพนักงาน</label><input bind:value={empCode} autocomplete="username" inputmode="numeric" /></div>
    <div class="sgf"><label>PIN 6 หลัก</label><input bind:value={pin} type="password" maxlength="6" inputmode="numeric" autocomplete="current-password" /></div>
    {#if err}<div class="merr">{err}</div>{/if}
    <div class="sgact">
      <button class="mbtn accent" disabled={busy}>{busy ? 'กำลังเข้าสู่ระบบ…' : 'เข้าสู่ระบบ'}</button>
    </div>
  </form>
</div>

<style>
  .login-wrap { min-height: 100vh; display: flex; align-items: center; justify-content: center; background: #F5F7FA; padding: 24px; }
  form.sgbox { width: min(420px, 100%); }
  .logo { width: 46px; height: 46px; border-radius: 12px; background: #0B3679; color: #fff; display: flex; align-items: center; justify-content: center; font-weight: 700; margin-bottom: 8px; }
</style>
