<script>
  import { fmtKB } from './lib/const.js'
  import { ovFileUrl } from './lib/api.js'
  export let overview = []
  let active = '', q = ''
  $: if (!active && overview.length) active = overview[0].key
  $: sec = overview.find((s) => s.key === active)
  const OV_INLINE = ['pdf', 'jpg', 'jpeg', 'png']
  const extOf = (f) => (f.ext || f.path.split('.').pop() || '').toLowerCase()
  const icon = (f) => {
    const e = extOf(f)
    return e === 'pdf' ? '📕' : ['jpg', 'jpeg', 'png'].includes(e) ? '🖼️' : ['xls', 'xlsx'].includes(e) ? '📊' : ['doc', 'docx'].includes(e) ? '📘' : '📄'
  }
</script>

<div class="cat-head"><h2 class="cat-title">📁 ภาพรวมองค์กร</h2><p class="cat-desc">ข้อมูลบริษัท บทบาทองค์กร SWOT และเอกสารทั่วไป — เปิดดู/ดาวน์โหลดไฟล์ต้นฉบับ</p></div>
<div class="subtabs">
  {#each overview as s}
    <button class="st-btn {s.key === active ? 'active' : ''}" on:click={() => (active = s.key)}>{s.icon || ''} {s.name} <span class="st-n">{s.groups.reduce((a, g) => a + g.items.length, 0)}</span></button>
  {/each}
</div>
<input class="cat-search" placeholder="🔍 ค้นหาชื่อไฟล์ในหมวดนี้..." bind:value={q} />
{#if sec}
  <p class="ov-sub">{sec.sub || ''}</p>
  {#each sec.groups as g}
    {@const files = g.items.filter((f) => !q || f.name.toLowerCase().includes(q.toLowerCase()))}
    {#if files.length}
      <div class="ov-group">
        <div class="ovg-head">{g.label} <span class="mut">({files.length})</span></div>
        {#each files as f}
          <div class="ov-row">
            <span class="ov-ic">{icon(f)}</span>
            <div class="ov-name">{f.name}<span class="mut"> · {fmtKB(f.size)}</span></div>
            <div class="ov-btns">
              {#if OV_INLINE.includes(extOf(f))}<a class="open" href={ovFileUrl(f.path)} target="_blank" rel="noreferrer">เปิดดู</a>{/if}
              <a class="open gh" href={ovFileUrl(f.path, 1)} download={f.name}>⬇ ดาวน์โหลด</a>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {/each}
{/if}
