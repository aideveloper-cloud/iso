<script>
  import { createEventDispatcher } from 'svelte'
  import { fmtKB, fileExt, fileKind, isWebEditable } from './lib/const.js'
  import { ovFileUrl } from './lib/api.js'
  import Icon from './Icon.svelte'
  export let overview = []
  export let jump = { n: 0, q: '', section: '' }
  const dispatch = createEventDispatcher()
  let active = '', q = ''
  let applied = 0
  $: if (!active && overview.length) active = overview[0].key
  $: if (jump.n && jump.n !== applied) {
    applied = jump.n
    if (jump.section) active = jump.section
    if (jump.q != null) q = jump.q
  }
  $: sec = overview.find((s) => s.key === active)
  const matchFile = (f) => !q || f.name.toLowerCase().includes(q.toLowerCase())
  const openFile = (f) => dispatch('preview', f)
</script>

<div class="cat-head"><h2 class="cat-title inline-flex items-center gap-2"><Icon name="folder" size={20} /> ภาพรวมองค์กร</h2><p class="cat-desc">ข้อมูลบริษัท บทบาทองค์กร SWOT และเอกสารทั่วไป — คลิกแถวเพื่อเปิดดูบนหน้าเว็บได้ทุกไฟล์</p></div>
<div class="subtabs">
  {#each overview as s}
    <button class="st-btn {s.key === active ? 'active' : ''}" on:click={() => (active = s.key)}>{s.icon || ''} {s.name} <span class="st-n">{s.groups.reduce((a, g) => a + g.items.length, 0)}</span></button>
  {/each}
</div>
<div class="relative mb-4"><span class="search-ic"><Icon name="search" size={16} /></span><input class="cat-search !mb-0 pl-[38px]" placeholder="ค้นหาชื่อไฟล์ในหมวดนี้..." bind:value={q} /></div>
{#if sec}
  <p class="ov-sub">{sec.sub || ''}</p>
  {#each sec.groups as g}
    {@const files = g.items.filter(matchFile)}
    {#if files.length}
      <div class="ov-group">
        <div class="ovg-head">{g.label} <span class="mut">({files.length})</span></div>
        {#each files as f}
          {@const e = fileExt(f)}
          {@const chip = fileKind(e)}
          <div class="ov-row clickrow" role="button" tabindex="0" on:click={() => openFile(f)} on:keydown={(ev) => ev.key === 'Enter' && openFile(f)}>
            <span class="ov-ic text-brand"><Icon name={chip.icon} size={20} /></span>
            <div class="ov-name">{f.name}<span class="mut"> · {fmtKB(f.size)}</span></div>
            <span class="tag {chip.c}">{chip.t}</span>
            <div class="ov-btns">
              <button class="open inline-flex items-center gap-1.5" on:click|stopPropagation={() => openFile(f)}><Icon name="eye" size={14} /> {isWebEditable(e) ? 'เปิด / แก้ไข' : 'เปิดดู'}</button>
              <a class="open gh inline-flex items-center gap-1.5" href={ovFileUrl(f.path, 1)} download={f.name} on:click|stopPropagation><Icon name="download" size={14} /> ดาวน์โหลด</a>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {/each}
  {#if !sec.groups.some((g) => g.items.some(matchFile))}
    <div class="empty-state"><Icon name="search" size={38} stroke={1.5} /><p>ไม่พบไฟล์ที่ตรงกับ “{q}”</p></div>
  {/if}
{/if}
