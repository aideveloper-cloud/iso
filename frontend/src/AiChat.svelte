<script>
  // แชทผู้ช่วย ISO (Agent) — ถามเรื่องขั้นตอน เอกสาร และงานค้างในระบบ · ตอบจากข้อมูลจริง (RAG + เครื่องมืออ่านข้อมูล) ดู api/src/aichat.rs
  //   อ่านอย่างเดียว: การอนุมัติ/ลงนาม/แก้เอกสาร ผู้ใช้ทำเองในเมนูตามปกติ · รหัสเอกสารในคำตอบกดเปิดเอกสารได้
  import { createEventDispatcher, tick } from 'svelte'
  import { api } from './lib/api.js'
  import Icon from './Icon.svelte'

  export let docs = []
  const dispatch = createEventDispatcher()

  let open = false, chats = [], chatId = null, msgs = [], input = '', busy = false, err = '', showList = false
  let listEl, inputEl
  const EXAMPLES = ['หาเอกสารขั้นตอนการปฏิบัติงานคลังสินค้า', 'ขั้นตอนขอแก้ไขเอกสารทำยังไง', 'ตอนนี้ฉันมีงานค้างอะไรบ้าง', 'CAR ที่ยังไม่ปิดมีอะไรบ้าง']
  $: known = new Set(docs.map((d) => d.code))

  async function toggle() {
    open = !open
    if (open) { await loadChats(); await tick(); inputEl?.focus() }
  }
  async function loadChats() { try { chats = await api.aiChats() } catch (e) { chats = [] } }
  async function pick(id) {
    showList = false; err = ''
    try { chatId = id; msgs = await api.aiChatGet(id); await scroll() } catch (e) { err = 'เปิดแชทไม่สำเร็จ: ' + e.message }
  }
  function newChat() { chatId = null; msgs = []; err = ''; showList = false; inputEl?.focus() }
  async function remove(id) {
    await api.aiChatDelete(id)
    if (id === chatId) newChat()
    loadChats()
  }
  async function scroll() { await tick(); if (listEl) listEl.scrollTop = listEl.scrollHeight }

  async function ask(text = input) {
    text = text.trim()
    if (!text || busy) return
    input = ''; err = ''; busy = true
    msgs = [...msgs, { role: 'user', content: text }]
    await scroll()
    let r
    try { r = await api.aiChat(chatId, text) } catch (e) { r = { ok: false, body: { error: 'เชื่อมต่อเซิร์ฟเวอร์ไม่ได้ — ลองใหม่อีกครั้ง' } } } finally { busy = false }
    if (r.body?.chatId) chatId = r.body.chatId
    if (!r.ok) { err = r.body.error || 'ผู้ช่วยตอบไม่สำเร็จ'; await scroll(); return }
    chatId = r.body.chatId
    msgs = [...msgs, { role: 'assistant', content: r.body.reply, sources: r.body.sources || [] }]
    await scroll()
    loadChats()
  }
  function onKey(e) { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); ask() } }

  /** แยกข้อความเป็นช่วงข้อความธรรมดา/รหัสเอกสาร [XX-YYY-000] ที่กดเปิดได้ (ไม่ใช้ HTML จาก AI) */
  function parts(text) {
    const out = []
    // รหัสเอกสารทั้งแบบ [QP-DCC-001] และไม่มีวงเล็บ — กดเปิดได้เฉพาะรหัสที่มีในทะเบียน
    const re = /\[?([A-Z]{2,4}-[A-Z0-9]{1,6}-\d{2,4}[A-Z0-9-]*)\]?/g
    let last = 0, m
    while ((m = re.exec(text))) {
      if (m.index > last) out.push({ t: text.slice(last, m.index) })
      out.push(known.has(m[1]) ? { code: m[1] } : { t: m[0] })
      last = re.lastIndex
    }
    if (last < text.length) out.push({ t: text.slice(last) })
    return out
  }
  function openDoc(code) { dispatch('open', code) }
  function openOv(path) { dispatch('openov', path) }
  /** ลิงก์ที่คัดลอกส่งต่อได้ (เปิดแล้วเข้าเอกสาร/ไฟล์นั้นหลังเข้าสู่ระบบ) */
  const docHref = (code) => '#doc=' + encodeURIComponent(code)
  const ovHref = (path) => '#ov=' + encodeURIComponent(path)
</script>

<button class="aic-fab" title="ถามผู้ช่วย ISO" on:click={toggle}><Icon name={open ? 'x' : 'message'} size={22} /></button>

{#if open}
  <div class="aic-panel" role="dialog" aria-label="ผู้ช่วย ISO">
    <header class="aic-head">
      <b>ผู้ช่วย ISO</b>
      <span class="mut text-[11.5px]">ตอบจากเอกสารและข้อมูลในระบบ · อ่านอย่างเดียว</span>
      <span class="grow"></span>
      <button class="mbtn gh !px-2" title="แชทก่อนหน้า" on:click={() => (showList = !showList)}><Icon name="clock" size={14} /></button>
      <button class="mbtn gh !px-2" title="แชทใหม่" on:click={newChat}><Icon name="plus" size={14} /></button>
    </header>

    {#if showList}
      <div class="aic-list">
        {#each chats as c}
          <div class="aic-li" class:cur={c.id === chatId}>
            <button class="grow text-left" on:click={() => pick(c.id)}>{c.title}</button>
            <button class="mut" title="ลบแชท" on:click={() => remove(c.id)}><Icon name="x" size={13} /></button>
          </div>
        {:else}
          <p class="mut p-3">ยังไม่มีแชท</p>
        {/each}
      </div>
    {:else}
      <div class="aic-msgs" bind:this={listEl}>
        {#if !msgs.length}
          <div class="aic-empty">
            <p>หาเอกสาร ถามเรื่องขั้นตอนงาน ข้อกำหนด ISO หรืองานค้างของคุณได้เลย</p>
            {#each EXAMPLES as ex}<button class="aic-ex" on:click={() => ask(ex)}>{ex}</button>{/each}
          </div>
        {/if}
        {#each msgs as m}
          <div class="aic-msg {m.role}">
            <div class="aic-bubble">{#each parts(m.content) as p}{#if p.code}<a class="aic-code" href={docHref(p.code)} on:click|preventDefault={() => openDoc(p.code)}>{p.code}</a>{:else}{p.t}{/if}{/each}</div>
            {#if m.sources?.length}
              <div class="aic-src">
                {#each m.sources as s}
                  {#if s.source === 'ov'}<a class="aic-code" href={ovHref(s.ref)} title="เปิดไฟล์ในภาพรวมองค์กร" on:click|preventDefault={() => openOv(s.ref)}><Icon name="folder" size={11} /> {s.title}</a>
                  {:else if known.has(s.ref)}<a class="aic-code" href={docHref(s.ref)} title={s.title} on:click|preventDefault={() => openDoc(s.ref)}>{s.ref}</a>{/if}
                {/each}
              </div>
            {/if}
          </div>
        {/each}
        {#if busy}<div class="aic-msg assistant"><div class="aic-bubble mut">กำลังค้นข้อมูล…</div></div>{/if}
        {#if err}<div class="merr mx-3">{err}</div>{/if}
      </div>
      <div class="aic-input">
        <textarea rows="2" bind:this={inputEl} bind:value={input} on:keydown={onKey} placeholder="พิมพ์คำถาม (Enter ส่ง · Shift+Enter ขึ้นบรรทัด)" maxlength="2000"></textarea>
        <button class="mbtn accent !px-3" disabled={busy || !input.trim()} on:click={() => ask()}><Icon name="send" size={15} /></button>
      </div>
    {/if}
  </div>
{/if}
