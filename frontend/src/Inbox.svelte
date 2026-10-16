<script>
  // งานของฉัน — กระดิ่งแจ้งเตือนบนแถบด้านบน: รวมงานที่รอผู้ใช้จากทุกโมดูล + ตั้งค่าอีเมลสรุปงานรายวัน
  import { onMount, onDestroy, createEventDispatcher } from 'svelte'
  import { api } from './lib/api.js'
  import Icon from './Icon.svelte'
  export let refresh = 0
  const dispatch = createEventDispatcher()

  const KIND = { dcr: 'DCR', form: 'แบบฟอร์ม', ack: 'รับทราบ', capa: 'CAR/PAR', mr: 'ทบทวนฯ', ia: 'ตรวจติดตาม', kpi: 'KPI', risk: 'ความเสี่ยง', cal: 'สอบเทียบ', sup: 'ผู้ขาย' }
  const LEVEL = { overdue: 'เลยกำหนด', soon: 'ใกล้ครบกำหนด', todo: 'รอดำเนินการ' }

  let open = false, data = null, err = ''
  let showSettings = false, email = '', emailOn = false, saving = false, setMsg = '', setErr = ''
  async function load() {
    try {
      data = await api.notify()
      if (data.error) throw new Error(data.error)
      err = ''
      if (!showSettings) { email = data.email; emailOn = data.emailOn }
    } catch (e) { err = 'โหลดงานไม่สำเร็จ: ' + e.message }
  }
  let timer
  onMount(() => { load(); timer = setInterval(load, 5 * 60e3) })
  onDestroy(() => clearInterval(timer))
  $: refresh, load()

  $: items = data?.items || []
  $: groups = ['overdue', 'soon', 'todo'].map((l) => [l, items.filter((x) => x.level === l)]).filter(([, xs]) => xs.length)
  $: badge = items.length

  function toggle() { open = !open; if (open) { load(); setMsg = ''; setErr = '' } }
  function go(it) { open = false; dispatch('go', { tab: it.tab, ref: it.ref, kind: it.kind }) }
  async function saveSettings() {
    saving = true; setMsg = ''; setErr = ''
    const r = await api.notifySettings(email, emailOn)
    saving = false
    if (!r.ok) { setErr = r.body.error || 'บันทึกไม่สำเร็จ'; return }
    setMsg = emailOn ? 'บันทึกแล้ว — จะได้รับสรุปงานทุกเช้าเมื่อมีงานค้าง' : 'บันทึกแล้ว — ปิดการรับอีเมล'
    load()
  }
  async function sendTest() {
    saving = true; setMsg = ''; setErr = ''
    const r = await api.notifyTest()
    saving = false
    if (!r.ok) { setErr = r.body.error || 'ส่งไม่สำเร็จ'; return }
    setMsg = 'ส่งอีเมลทดสอบไปที่ ' + r.body.to + ' แล้ว'
  }
</script>

<svelte:window on:keydown={(e) => e.key === 'Escape' && (open = false)} />

<div class="relative">
  <button class="bell {data?.overdue ? 'hot' : ''}" title="งานของฉัน" aria-label="งานของฉัน {badge} รายการ" on:click={toggle}>
    <Icon name="bell" size={18} />
    {#if badge}<span class="bell-n">{badge > 99 ? '99+' : badge}</span>{/if}
  </button>
  {#if open}
    <div class="bell-bg" on:mousedown={() => (open = false)}></div>
    <div class="bell-panel">
      <div class="flex items-center gap-2 px-3 py-2 border-b border-line">
        <b class="text-[14px]">งานของฉัน</b>
        {#if data}<span class="mut text-[12px]">{items.length} รายการ{#if data.overdue} · <span class="text-danger font-bold">เลยกำหนด {data.overdue}</span>{/if}</span>{/if}
        <span class="flex-1"></span>
        <button class="mbtn gh !py-0.5 inline-flex items-center gap-1" title="ตั้งค่าอีเมล" on:click={() => (showSettings = !showSettings)}><Icon name="mail" size={14} /> อีเมล</button>
      </div>
      {#if showSettings}
        <div class="px-3 py-2 border-b border-line bg-[#f8fafc]">
          <div class="sgf !mb-2"><label>อีเมลของฉัน</label><input type="email" bind:value={email} placeholder="name@kgarden.co.th" /></div>
          <label class="inline-flex items-center gap-2 text-[13px] cursor-pointer"><input type="checkbox" bind:checked={emailOn} /> รับอีเมลสรุปงานค้างทุกเช้า</label>
          {#if data && !data.smtp}<p class="mnote !mt-1">เซิร์ฟเวอร์ยังไม่ได้ตั้งค่าการส่งอีเมล (SMTP) — บันทึกไว้ได้ ระบบจะเริ่มส่งเมื่อผู้ดูแลตั้งค่าแล้ว</p>{/if}
          {#if setErr}<div class="merr !my-1">{setErr}</div>{/if}
          {#if setMsg}<div class="save-ok !my-1">{setMsg}</div>{/if}
          <div class="flex gap-2 mt-2">
            <button class="mbtn accent !py-1" disabled={saving} on:click={saveSettings}>บันทึก</button>
            {#if data?.smtp && data?.email}<button class="mbtn gh !py-1" disabled={saving} on:click={sendTest}>ส่งอีเมลทดสอบ</button>{/if}
          </div>
        </div>
      {/if}
      <div class="bell-list">
        {#if err}<div class="merr m-3">{err}</div>
        {:else if !data}<div class="loadwrap !py-6"><span class="spin"></span></div>
        {:else if !items.length}<div class="empty-state !py-8"><Icon name="circle-check" size={30} stroke={1.5} /><p>ไม่มีงานค้าง</p></div>
        {:else}
          {#each groups as [lvl, xs]}
            <div class="bell-sec l-{lvl}">{LEVEL[lvl]} ({xs.length})</div>
            {#each xs as it}
              <button class="bell-item" on:click={() => go(it)}>
                <span class="bell-kind">{KIND[it.kind] || it.kind}</span>
                <span class="min-w-0 flex-1">
                  <span class="block font-semibold text-[13px] leading-snug">{it.title}</span>
                  <span class="block mut text-[11.5px]">{it.ref}{it.detail ? ' · ' + it.detail : ''}</span>
                </span>
                {#if it.due}<span class="text-[11.5px] whitespace-nowrap {lvl === 'overdue' ? 'text-danger font-bold' : 'mut'}">{it.due}</span>{/if}
              </button>
            {/each}
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</div>
