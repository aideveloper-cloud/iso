<script>
  import { api } from './lib/api.js'
  export let token = ''
  let info = null
  let empCode = ''
  let pin = ''
  let err = ''
  let ok = ''
  let busy = false

  async function load() {
    info = await api.checkinInfo(token)
    if (info.error) err = info.error
  }
  load()

  async function submit() {
    err = ''; ok = ''; busy = true
    const res = await api.checkin({ token, empCode: empCode.trim(), pin: pin.trim() })
    busy = false
    if (!res.ok) { err = res.body.error || 'เช็คอินไม่ได้'; return }
    ok = `${res.body.name} เช็คอินแล้ว ${res.body.at}`
    pin = ''
  }
</script>

<div class="login-wrap">
  <form class="sgbox" on:submit|preventDefault={submit}>
    <h3>เช็คอินเข้าอบรม</h3>
    {#if info && !info.error}
      <div class="sgs">{info.title || 'ใบรายชื่อผู้เข้าร่วมอบรม'}</div>
      <p class="note">{info.trainDate} · {info.time}<br>{info.place || ''}</p>
      {#if info.expired}<div class="merr">เลยเวลาอบรมแล้ว เช็คอินไม่ได้</div>{/if}
    {/if}
    <div class="sgf"><label>รหัสพนักงาน</label><input bind:value={empCode} inputmode="numeric" /></div>
    <div class="sgf"><label>PIN 6 หลัก</label><input bind:value={pin} type="password" maxlength="6" inputmode="numeric" /></div>
    {#if err}<div class="merr">{err}</div>{/if}
    {#if ok}<div class="save-ok">{ok}</div>{/if}
    <div class="sgact">
      <button class="mbtn accent" disabled={busy || info?.expired}>{busy ? 'กำลังบันทึก…' : 'ยืนยันการเข้าร่วม'}</button>
    </div>
  </form>
</div>

<style>
  .login-wrap { min-height: 100vh; display: flex; align-items: center; justify-content: center; background: #F5F7FA; padding: 16px; }
  form.sgbox { width: min(440px, 100%); }
  .sgf input { min-height: 46px; font-size: 18px; }
</style>
