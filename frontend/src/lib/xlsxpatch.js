// Patch cell values inside the ORIGINAL xlsx package: only the affected
// <row>/<c> XML is rewritten, every other zip entry keeps its exact bytes.
import { unzipSync, zipSync } from 'fflate'
import { resolveSheets, bytesToText, colIndex, colName } from './xlsxview.js'

const enc = new TextEncoder()
const ATTRS = '(?:[^>"\']|"[^"]*"|\'[^\']*\')*?'
const ROW_RE = () => new RegExp(`<row\\b${ATTRS}(?:/>|>[\\s\\S]*?</row>)`, 'g')
const CELL_RE = () => new RegExp(`<c\\b${ATTRS}(?:/>|>[\\s\\S]*?</c>)`, 'g')
const OPEN_RE = new RegExp(`^<(row|c)\\b(${ATTRS})(/?)>`)

const ILLEGAL = /[\u0000-\u0008\u000B\u000C\u000E-\u001F￾￿]|[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g

export function escapeXmlText(s) {
  return String(s)
    .replace(/\r\n?/g, '\n')
    .replace(ILLEGAL, '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function parseAttrs(s) {
  const out = []
  const re = /([^\s=/>]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g
  let m
  while ((m = re.exec(s))) out.push([m[1], m[2] !== undefined ? m[2] : m[3]])
  return out
}
const getA = (attrs, n) => {
  const a = attrs.find((x) => x[0] === n)
  return a ? a[1] : undefined
}
const attrStr = (attrs) => attrs.map(([n, v]) => ` ${n}="${v}"`).join('')

function parseCellRef(ref) {
  const m = /^([A-Za-z]+)(\d+)$/.exec(ref || '')
  return m ? { c: colIndex(m[1]), r: +m[2] } : null
}

const NUM_RE = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/
function parseNumber(text) {
  let t = text.trim()
  if (/^[+-]?\d{1,3}(,\d{3})+(\.\d+)?$/.test(t)) t = t.replace(/,/g, '')
  if (!NUM_RE.test(t)) return null
  const n = Number(t)
  return Number.isFinite(n) ? n : null
}

/** numFmtId per cellXfs index (49 = text) */
function xfNumFmts(files) {
  const b = files['xl/styles.xml']
  if (!b) return []
  const s = bytesToText(b)
  const m = /<cellXfs\b[^>]*>([\s\S]*?)<\/cellXfs>/.exec(s)
  if (!m) return []
  const out = []
  const re = new RegExp(`<xf\\b(${ATTRS})(?:/>|>)`, 'g')
  let x
  while ((x = re.exec(m[1]))) out.push(+(getA(parseAttrs(x[1]), 'numFmtId') || 0))
  return out
}

/** build the new <c> element text */
function buildCell(ref, oldAttrs, hadNumeric, value, numFmtId) {
  const attrs = (oldAttrs || []).filter(([n]) => n !== 't' && n !== 'r')
  const head = [['r', ref], ...attrs]
  const clean = String(value).replace(ILLEGAL, '')
  if (clean === '') return `<c${attrStr(head)}/>`
  const num = hadNumeric && numFmtId !== 49 ? parseNumber(clean) : null
  if (num !== null) return `<c${attrStr(head)}><v>${String(num)}</v></c>`
  return `<c${attrStr([...head, ['t', 'inlineStr']])}><is><t xml:space="preserve">${escapeXmlText(clean)}</t></is></c>`
}

function patchSheetXml(xml, edits, ctx) {
  if (/<sheetData\s*\/>/.test(xml)) xml = xml.replace(/<sheetData\s*\/>/, '<sheetData></sheetData>')
  const sdOpen = /<sheetData\b[^>]*>/.exec(xml)
  const sdEnd = xml.lastIndexOf('</sheetData>')
  if (!sdOpen || sdEnd < 0) throw new Error('xlsx: sheetData not found (unsupported sheet xml)')
  const innerStart = sdOpen.index + sdOpen[0].length
  const inner = xml.slice(innerStart, sdEnd)

  // rows
  const rows = []
  let prev = 0
  const rre = ROW_RE()
  let m
  while ((m = rre.exec(inner))) {
    const om = OPEN_RE.exec(m[0])
    const attrs = parseAttrs(om[2])
    const rn = getA(attrs, 'r') ? +getA(attrs, 'r') : prev + 1
    prev = rn
    rows.push({ rn, start: m.index, end: m.index + m[0].length, text: m[0] })
  }
  // style lookup for existing rows (rowNum -> Map(col -> s))
  const styleMap = new Map()
  const indexRowStyles = (rn, rowText) => {
    const mm = new Map()
    const cre = CELL_RE()
    let c
    const body = rowText.replace(OPEN_RE, '')
    while ((c = cre.exec(body))) {
      const a = parseAttrs(OPEN_RE.exec(c[0])[2])
      const pr = parseCellRef(getA(a, 'r'))
      if (pr && getA(a, 's') !== undefined) mm.set(pr.c, getA(a, 's'))
    }
    styleMap.set(rn, mm)
  }
  for (const r of rows) indexRowStyles(r.rn, r.text)
  const rowNums = () => [...styleMap.keys()].sort((a, b) => a - b)
  const prevRowStyle = (rn, col) => {
    const ks = rowNums().filter((k) => k < rn).reverse()
    for (const k of ks) {
      const s = styleMap.get(k).get(col)
      if (s !== undefined) return s
    }
    return undefined
  }

  // group edits by row (dedupe: last wins)
  const byRow = new Map()
  for (const e of edits) {
    const rn = e.r + 1
    if (!byRow.has(rn)) byRow.set(rn, new Map())
    byRow.get(rn).set(e.c, e.value)
  }
  const replacements = [] // {start,end,text}
  const inserts = new Map() // pos -> [{rn,text}]
  const existing = new Map(rows.map((r) => [r.rn, r]))
  let ext = null // {minR,maxR,minC,maxC}
  const extend = (rn, c) => {
    if (!ext) ext = { minR: rn, maxR: rn, minC: c, maxC: c }
    else { ext.minR = Math.min(ext.minR, rn); ext.maxR = Math.max(ext.maxR, rn); ext.minC = Math.min(ext.minC, c); ext.maxC = Math.max(ext.maxC, c) }
  }

  const editRow = (rn, rowText, cellEdits, isNew) => {
    let openAttrs, body
    if (isNew) { openAttrs = [['r', String(rn)]]; body = '' }
    else {
      const om = OPEN_RE.exec(rowText)
      openAttrs = parseAttrs(om[2]).filter(([n]) => n !== 'spans')
      body = om[3] === '/' ? '' : rowText.slice(om[0].length, rowText.length - '</row>'.length)
    }
    const rowStyle = getA(openAttrs, 'customFormat') === '1' || getA(openAttrs, 'customFormat') === 'true' ? getA(openAttrs, 's') : undefined
    const cols = [...cellEdits.keys()].sort((a, b) => a - b)
    for (const col of cols) {
      const value = cellEdits.get(col)
      const ref = colName(col) + rn
      // scan cells
      const cre = CELL_RE()
      let found = null, leftS, cm
      while ((cm = cre.exec(body))) {
        const a = parseAttrs(OPEN_RE.exec(cm[0])[2])
        const pr = parseCellRef(getA(a, 'r'))
        if (!pr) continue
        if (pr.c === col) { found = { start: cm.index, end: cm.index + cm[0].length, text: cm[0], attrs: a }; break }
        if (pr.c < col && getA(a, 's') !== undefined) leftS = getA(a, 's')
        if (pr.c > col) { found = { insertAt: cm.index }; break }
      }
      if (found && found.text) {
        const t = getA(found.attrs, 't')
        const inner = found.text.replace(OPEN_RE, '')
        const fm = new RegExp(`<f\\b(${ATTRS})(?:/>|>[\\s\\S]*?</f>)`).exec(inner)
        if (fm) {
          const ft = getA(parseAttrs(fm[1]), 't')
          if (getA(parseAttrs(fm[1]), 'ref') !== undefined && (ft === 'shared' || ft === 'array')) {
            ctx.skipped.push({ ref, reason: 'shared/array formula master' })
            continue
          }
          ctx.droppedFormula = true
        }
        const hadNumeric = t === undefined || t === 'n'
        const s = getA(found.attrs, 's')
        const nf = s !== undefined ? ctx.numFmts[+s] ?? 0 : 0
        const cell = buildCell(ref, found.attrs, hadNumeric, value, nf)
        body = body.slice(0, found.start) + cell + body.slice(found.end)
        if (String(value) !== '') extend(rn, col)
      } else {
        let s = leftS
        if (s === undefined) s = prevRowStyle(rn, col)
        if (s === undefined) s = rowStyle
        const nf = s !== undefined ? ctx.numFmts[+s] ?? 0 : 0
        const attrs = s !== undefined ? [['s', s]] : []
        const cell = buildCell(ref, attrs, true, value, nf)
        const at = found ? found.insertAt : body.length
        body = body.slice(0, at) + cell + body.slice(at)
        if (String(value) !== '') extend(rn, col)
      }
    }
    const idx = new Map()
    {
      const cre = CELL_RE()
      let cm
      while ((cm = cre.exec(body))) {
        const a = parseAttrs(OPEN_RE.exec(cm[0])[2])
        const pr = parseCellRef(getA(a, 'r'))
        if (pr && getA(a, 's') !== undefined) idx.set(pr.c, getA(a, 's'))
      }
    }
    styleMap.set(rn, idx)
    return `<row${attrStr(openAttrs)}>${body}</row>`
  }

  const sortedEdits = [...byRow.keys()].sort((a, b) => a - b)
  for (const rn of sortedEdits) {
    const ex = existing.get(rn)
    if (ex) {
      const text = editRow(rn, ex.text, byRow.get(rn), false)
      if (text !== ex.text) replacements.push({ start: ex.start, end: ex.end, text })
    } else {
      const text = editRow(rn, '', byRow.get(rn), true)
      const nxt = rows.find((r) => r.rn > rn)
      const pos = nxt ? nxt.start : inner.length
      if (!inserts.has(pos)) inserts.set(pos, [])
      inserts.get(pos).push({ rn, text })
    }
  }
  for (const [pos, list] of inserts) {
    list.sort((a, b) => a.rn - b.rn)
    replacements.push({ start: pos, end: pos, text: list.map((x) => x.text).join('') })
  }
  if (!replacements.length) return xml
  replacements.sort((a, b) => b.start - a.start)
  let newInner = inner
  for (const r of replacements) newInner = newInner.slice(0, r.start) + r.text + newInner.slice(r.end)
  let out = xml.slice(0, innerStart) + newInner + xml.slice(sdEnd)

  // dimension
  if (ext) {
    out = out.replace(/<dimension\b[^>]*?\bref="([^"]*)"/, (all, ref) => {
      const [a, b] = ref.split(':')
      const p1 = parseCellRef(a), p2 = parseCellRef(b || a)
      if (!p1 || !p2) return all
      const minR = Math.min(p1.r, ext.minR), maxR = Math.max(p2.r, ext.maxR)
      const minC = Math.min(p1.c, ext.minC), maxC = Math.max(p2.c, ext.maxC)
      const nref = `${colName(minC)}${minR}:${colName(maxC)}${maxR}`
      const norm = nref.split(':')[0] === nref.split(':')[1] ? nref.split(':')[0] : nref
      return all.replace(`ref="${ref}"`, `ref="${norm}"`)
    })
  }
  return out
}

/**
 * @param {Uint8Array} origBytes original .xlsx
 * @param {{sheet:string,r:number,c:number,value:string}[]} edits 0-based r/c
 * @returns {Uint8Array} patched .xlsx
 */
export function patchXlsx(origBytes, edits) {
  const files = unzipSync(origBytes)
  const names = Object.keys(files)
  const { sheets } = resolveSheets(files)
  const changed = new Map()
  const removed = new Set()
  const ctx = { numFmts: xfNumFmts(files), droppedFormula: false, skipped: [] }

  const bySheet = new Map()
  for (const e of edits || []) {
    const sh = sheets.find((s) => s.name === e.sheet)
    if (!sh) throw new Error(`xlsx: sheet not found: ${e.sheet}`)
    if (!bySheet.has(sh.path)) bySheet.set(sh.path, [])
    bySheet.get(sh.path).push(e)
  }
  for (const [path, list] of bySheet) {
    const xml = bytesToText(files[path])
    const out = patchSheetXml(xml, list, ctx)
    if (out !== xml) changed.set(path, enc.encode(out))
  }

  if (ctx.droppedFormula) {
    if (files['xl/calcChain.xml']) removed.add('xl/calcChain.xml')
    const ct = files['[Content_Types].xml']
    if (ct) {
      const s = bytesToText(ct)
      const n = s.replace(/<Override\b[^>]*calcChain[^>]*>/g, "")
      if (n !== s) changed.set('[Content_Types].xml', enc.encode(n))
    }
    const rl = files['xl/_rels/workbook.xml.rels']
    if (rl) {
      const s = bytesToText(rl)
      const n = s.replace(/<Relationship\b[^>]*calcChain[^>]*>/g, "")
      if (n !== s) changed.set('xl/_rels/workbook.xml.rels', enc.encode(n))
    }
    const wbx = bytesToText(files['xl/workbook.xml'])
    let n
    if (/<calcPr\b/.test(wbx)) {
      n = wbx.replace(/<calcPr\b([^>]*?)(\/?)>/, (all, a, sc) => {
        const a2 = /\bfullCalcOnLoad=/.test(a) ? a.replace(/\bfullCalcOnLoad="[^"]*"/, 'fullCalcOnLoad="1"') : a + ' fullCalcOnLoad="1"'
        return `<calcPr${a2}${sc}>`
      })
    } else {
      const tail = /<(oleSize|customWorkbookViews|pivotCaches|smartTagPr|smartTagTypes|webPublishing|fileRecoveryPr|webPublishObjects|extLst)\b|<\/workbook>/.exec(wbx)
      n = tail ? wbx.slice(0, tail.index) + '<calcPr fullCalcOnLoad="1"/>' + wbx.slice(tail.index) : wbx
    }
    if (n !== wbx) changed.set('xl/workbook.xml', enc.encode(n))
  }
  if (ctx.skipped.length && typeof console !== 'undefined') console.warn('xlsxpatch: skipped', ctx.skipped)

  const order = names.filter((n) => !removed.has(n))
  const ci = order.indexOf('[Content_Types].xml')
  if (ci > 0) order.unshift(...order.splice(ci, 1))
  const out = {}
  const mtime = new Date(2020, 0, 1)
  for (const n of order) {
    const data = changed.get(n) || files[n]
    const level = /\.(png|jpe?g|gif|zip|xlsx|docx|mp4|bin)$/i.test(n) ? 0 : 6
    out[n] = [data, { level, mtime }]
  }
  return zipSync(out)
}
