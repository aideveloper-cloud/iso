<script>
  import { createEventDispatcher } from 'svelte'
  import { colName } from './lib/xlsxview.js'

  export let model
  export let editing = false

  const dispatch = createEventDispatcher()
  const BLANK = { text: '', raw: '', kind: 'blank', formula: null, css: '', editable: true }

  let cur = {} // "r,c" -> current text (only touched cells)
  let lastModel = null
  $: if (model !== lastModel) { lastModel = model; cur = {} }

  let plain = true
  try {
    const t = document.createElement('div')
    t.contentEditable = 'plaintext-only'
    plain = t.contentEditable === 'plaintext-only'
  } catch { plain = false }

  $: totalW = 44 + (model ? model.colWidths.reduce((a, b) => a + b, 0) : 0)
  $: skipKeys = (() => {
    const s = new Set()
    for (const m of model?.merges || []) for (let r = m.r; r < m.r + m.rs; r++) for (let c = m.c; c < m.c + m.cs; c++) if (r !== m.r || c !== m.c) s.add(r + ',' + c)
    return s
  })()

  function cellAt(r, c) {
    return model.cells[r][c] || BLANK
  }
  function shown(cur, r, c, cell) {
    const v = cur[r + ',' + c]
    return v === undefined ? cell.text : v
  }
  function differs(cell, v) {
    return v !== cell.text && v !== String(cell.raw)
  }

  export function getEdits() {
    const out = []
    for (const k of Object.keys(cur)) {
      const [r, c] = k.split(',').map(Number)
      const cell = model.cells[r]?.[c] || BLANK
      if (differs(cell, cur[k])) out.push({ r, c, value: cur[k] })
    }
    return out.sort((a, b) => a.r - b.r || a.c - b.c)
  }

  function readValue(el) {
    let v = el.innerText ?? el.textContent ?? ''
    v = v.replace(/\r/g, '')
    if (!plain) v = v.replace(/\n$/, '')
    return v
  }

  // action: sets initial text once; the DOM is the source of truth while editing
  function editable(node, text) {
    node.contentEditable = plain ? 'plaintext-only' : 'true'
    node.textContent = text
    return {}
  }

  function onInput(e, r, c, cell) {
    const value = readValue(e.currentTarget)
    cur[r + ',' + c] = value
    const changed = differs(cell, value)
    dispatch('change', { r, c, value: changed ? value : cell.text, changed })
  }

  function focusCell(r, c) {
    const el = document.querySelector(`[data-xl="${uid}"] [data-r="${r}"][data-c="${c}"]`)
    if (el) { el.focus(); return true }
    return false
  }
  const uid = Math.random().toString(36).slice(2)

  function onKey(e, r, c, cell) {
    if (e.key === 'Enter') {
      const wrapped = /pre-wrap/.test(cell.css)
      const newline = e.altKey || (wrapped && !e.ctrlKey && !e.metaKey)
      e.preventDefault()
      if (newline) {
        document.execCommand('insertText', false, '\n')
        if (!plain) e.currentTarget.dispatchEvent(new Event('input'))
      } else {
        for (let rr = r + (e.shiftKey ? -1 : 1); rr >= 0 && rr < model.nRows; rr += e.shiftKey ? -1 : 1) if (focusCell(rr, c)) break
      }
    } else if (e.key === 'Escape') {
      e.currentTarget.textContent = cell.text
      delete cur[r + ',' + c]
      cur = cur
      dispatch('change', { r, c, value: cell.text, changed: false })
    } else if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && !/pre-wrap/.test(cell.css)) {
      e.preventDefault()
      const d = e.key === 'ArrowDown' ? 1 : -1
      for (let rr = r + d; rr >= 0 && rr < model.nRows; rr += d) if (focusCell(rr, c)) break
    }
  }

  function onPaste(e) {
    if (plain) return
    e.preventDefault()
    const t = (e.clipboardData || window.clipboardData).getData('text/plain')
    document.execCommand('insertText', false, t)
  }
  function onFocus(e) {
    const el = e.currentTarget
    const sel = window.getSelection()
    const range = document.createRange()
    range.selectNodeContents(el)
    range.collapse(false)
    sel.removeAllRanges()
    sel.addRange(range)
  }
</script>

{#if model}
  <div class="xl-wrap" data-xl={uid}>
    <table class="xl-grid" style="width:{totalW}px">
      <colgroup>
        <col style="width:44px" />
        {#each model.colWidths as w}<col style="width:{w}px" />{/each}
      </colgroup>
      <thead>
        <tr>
          <th class="xl-corner"></th>
          {#each model.colWidths as w, c}
            <th class="xl-colh" class:xl-hid={w === 0}>{colName(c)}</th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each model.cells as row, r}
          {@const h = model.rowHeights[r]}
          <tr style={h === 0 ? 'display:none' : `height:${h ?? model.defaultRowHeight}px`}>
            <th class="xl-rowh">{r + 1}</th>
            {#each row as cell, c}
              {#if !skipKeys.has(r + ',' + c)}
                {@const cl = cell || BLANK}
                <td
                  colspan={cl.colspan || 1}
                  rowspan={cl.rowspan || 1}
                  style="{cell ? cl.css : `border:1px solid #e5e7eb;white-space:nowrap`}"
                  class:xl-edit={editing && cl.editable}
                  class:xl-dirty={cur[r + ',' + c] !== undefined && differs(cl, cur[r + ',' + c])}
                  class:xl-ro={cl.kind === 'f'}
                  title={cl.formula ? '=' + cl.formula : null}
                >
                  {#if editing && cl.editable}
                    <div
                      class="xl-ed" role="textbox" tabindex="0" aria-multiline="true"
                      data-r={r}
                      data-c={c}
                      spellcheck="false"
                      use:editable={shown(cur, r, c, cl)}
                      on:input={(e) => onInput(e, r, c, cl)}
                      on:keydown={(e) => onKey(e, r, c, cl)}
                      on:paste={onPaste}
                      on:focus={onFocus}
                    ></div>
                  {:else}{shown(cur, r, c, cl)}{/if}
                </td>
              {/if}
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
    {#if model.truncated}
      <div class="xl-trunc">แสดงเฉพาะ {model.nRows} แถว × {model.nCols} คอลัมน์แรกของชีตนี้</div>
    {/if}
  </div>
{/if}

<style>
  .xl-wrap {
    overflow: auto;
    max-width: 100%;
    max-height: 100%;
    background: #fff;
    position: relative;
  }
  .xl-grid {
    table-layout: fixed;
    border-collapse: collapse;
    font-family: Tahoma, Sarabun, sans-serif;
    font-size: 11pt;
    line-height: 1.25;
    color: #000;
    background: #fff;
  }
  .xl-grid td {
    box-sizing: border-box;
    padding: 0 2px;
    border: 1px solid #e5e7eb;
    overflow: hidden;
    white-space: nowrap;
    vertical-align: bottom;
    position: relative;
  }
  .xl-ed {
    min-height: 1.25em;
    outline: none;
    cursor: text;
    white-space: inherit;
    word-break: inherit;
  }
  .xl-edit:focus-within {
    outline: 2px solid #3b82f6;
    outline-offset: -2px;
    overflow: visible;
    z-index: 2;
  }
  .xl-dirty {
    box-shadow: inset 0 0 0 9999px rgba(250, 204, 21, 0.18);
  }
  .xl-ro {
    cursor: default;
  }
  .xl-grid th {
    position: sticky;
    background: #f3f4f6;
    color: #6b7280;
    font: 600 11px Tahoma, Sarabun, sans-serif;
    text-align: center;
    padding: 0;
    border: none;
    box-shadow: inset 0 -1px 0 #d1d5db, inset -1px 0 0 #d1d5db;
    user-select: none;
    overflow: hidden;
  }
  .xl-colh {
    top: 0;
    height: 20px;
    z-index: 3;
  }
  .xl-rowh {
    left: 0;
    z-index: 2;
  }
  .xl-corner {
    top: 0;
    left: 0;
    z-index: 4;
    height: 20px;
  }
  .xl-hid {
    width: 0;
  }
  .xl-trunc {
    position: sticky;
    left: 0;
    padding: 6px 10px;
    font: 12px Tahoma, Sarabun, sans-serif;
    color: #92400e;
    background: #fffbeb;
  }
</style>
