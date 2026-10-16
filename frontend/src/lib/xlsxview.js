// Faithful xlsx -> view model (merges, widths, heights, styles, number formats).
// Pure JS (no DOM): works in browser and node.
import * as XLSX from 'xlsx'
import { unzipSync } from 'fflate'

export const MAX_ROWS = 300
export const MAX_COLS = 60
export const GRID_COLOR = '#e5e7eb'

/* ---------------- tiny XML tree parser ---------------- */
const ENT = { lt: '<', gt: '>', amp: '&', quot: '"', apos: "'" }
function decodeEnt(s) {
  if (s.indexOf('&') < 0) return s
  return s.replace(/&(#x[0-9a-fA-F]+|#\d+|lt|gt|amp|quot|apos);/g, (m, e) => {
    if (e[0] === '#') {
      const cp = e[1] === 'x' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10)
      try { return String.fromCodePoint(cp) } catch { return '' }
    }
    return ENT[e]
  })
}
function decodeXlsxEsc(s) {
  return s.indexOf('_x') < 0 ? s : s.replace(/_x([0-9A-Fa-f]{4})_/g, (m, h) => String.fromCharCode(parseInt(h, 16)))
}
const ATTR_RE = /([^\s=/>]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g
const TOKEN_RE = /<!--[\s\S]*?-->|<\?[\s\S]*?\?>|<!\[CDATA\[([\s\S]*?)\]\]>|<(\/?)([^\s/>]+)((?:[^>"']|"[^"]*"|'[^']*')*?)(\/?)>|([^<]+)/g

/** returns the document element: { tag(local name), attrs, children[], text } */
export function parseXml(s) {
  const root = { tag: '#root', attrs: {}, children: [], text: '' }
  const stack = [root]
  TOKEN_RE.lastIndex = 0
  let m
  while ((m = TOKEN_RE.exec(s))) {
    const top = stack[stack.length - 1]
    if (m[1] !== undefined) { top.text += m[1]; continue }
    if (m[3] !== undefined) {
      if (m[2]) { if (stack.length > 1) stack.pop(); continue }
      const attrs = {}
      if (m[4]) {
        ATTR_RE.lastIndex = 0
        let a
        while ((a = ATTR_RE.exec(m[4]))) attrs[a[1]] = decodeEnt(a[2] !== undefined ? a[2] : a[3])
      }
      const node = { tag: m[3].replace(/^.*:/, ''), attrs, children: [], text: '' }
      top.children.push(node)
      if (!m[5]) stack.push(node)
    } else if (m[6] !== undefined) {
      top.text += decodeEnt(m[6])
    }
  }
  return root.children[0] || root
}
const kids = (n, tag) => (n ? n.children.filter((c) => c.tag === tag) : [])
const kid = (n, tag) => (n ? n.children.find((c) => c.tag === tag) : undefined)
const attrAny = (n, suffix) => {
  for (const k in n.attrs) if (k === suffix || k.endsWith(':' + suffix)) return n.attrs[k]
}

export function bytesToText(u8) {
  return new TextDecoder('utf-8', { ignoreBOM: true }).decode(u8)
}

/* ---------------- workbook structure ---------------- */
function normPath(p) {
  const out = []
  for (const seg of p.split('/')) {
    if (seg === '..') out.pop()
    else if (seg && seg !== '.') out.push(seg)
  }
  return out.join('/')
}

/** files: unzipSync result. -> { sheets:[{name,path,hidden}], date1904 } */
export function resolveSheets(files) {
  const wbBytes = files['xl/workbook.xml']
  if (!wbBytes) throw new Error('xlsx: xl/workbook.xml not found')
  const wb = parseXml(bytesToText(wbBytes))
  const relsBytes = files['xl/_rels/workbook.xml.rels']
  const rels = {}
  if (relsBytes) {
    for (const r of kids(parseXml(bytesToText(relsBytes)), 'Relationship')) {
      const t = r.attrs.Target || ''
      rels[r.attrs.Id] = t[0] === '/' ? normPath(t) : normPath('xl/' + t)
    }
  }
  const sheets = []
  for (const s of kids(kid(wb, 'sheets'), 'sheet')) {
    const rid = attrAny(s, 'id')
    const path = rels[rid]
    if (!path) continue
    sheets.push({ name: s.attrs.name, path, hidden: s.attrs.state === 'hidden' || s.attrs.state === 'veryHidden' })
  }
  const pr = kid(wb, 'workbookPr')
  return { sheets, date1904: !!pr && (pr.attrs.date1904 === '1' || pr.attrs.date1904 === 'true') }
}

/* ---------------- colors ---------------- */
const INDEXED = [
  '000000', 'FFFFFF', 'FF0000', '00FF00', '0000FF', 'FFFF00', 'FF00FF', '00FFFF',
  '000000', 'FFFFFF', 'FF0000', '00FF00', '0000FF', 'FFFF00', 'FF00FF', '00FFFF',
  '800000', '008000', '000080', '808000', '800080', '008080', 'C0C0C0', '808080',
  '9999FF', '993366', 'FFFFCC', 'CCFFFF', '660066', 'FF8080', '0066CC', 'CCCCFF',
  '000080', 'FF00FF', 'FFFF00', '00FFFF', '800080', '800000', '008080', '0000FF',
  '00CCFF', 'CCFFFF', 'CCFFCC', 'FFFF99', '99CCFF', 'FF99CC', 'CC99FF', 'FFCC99',
  '3366FF', '33CCCC', '99CC00', 'FFCC00', 'FF9900', 'FF6600', '666699', '969696',
  '003366', '339966', '003300', '333300', '993300', '993366', '333399', '333333',
]
const DEFAULT_THEME = ['FFFFFF', '000000', 'EEECE1', '1F497D', '4F81BD', 'C0504D', '9BBB59', '8064A2', '4BACC6', 'F79646']

function applyTint(hex, tint) {
  const r = parseInt(hex.slice(0, 2), 16) / 255, g = parseInt(hex.slice(2, 4), 16) / 255, b = parseInt(hex.slice(4, 6), 16) / 255
  const mx = Math.max(r, g, b), mn = Math.min(r, g, b)
  let h = 0, s = 0, l = (mx + mn) / 2
  if (mx !== mn) {
    const d = mx - mn
    s = l > 0.5 ? d / (2 - mx - mn) : d / (mx + mn)
    if (mx === r) h = (g - b) / d + (g < b ? 6 : 0)
    else if (mx === g) h = (b - r) / d + 2
    else h = (r - g) / d + 4
    h /= 6
  }
  l = tint < 0 ? l * (1 + tint) : l * (1 - tint) + tint
  const hue = (p, q, t) => {
    if (t < 0) t += 1
    if (t > 1) t -= 1
    if (t < 1 / 6) return p + (q - p) * 6 * t
    if (t < 1 / 2) return q
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6
    return p
  }
  let R, G, B
  if (s === 0) R = G = B = l
  else {
    const q = l < 0.5 ? l * (1 + s) : l + s - l * s, p = 2 * l - q
    R = hue(p, q, h + 1 / 3); G = hue(p, q, h); B = hue(p, q, h - 1 / 3)
  }
  const x = (v) => Math.max(0, Math.min(255, Math.round(v * 255))).toString(16).padStart(2, '0')
  return (x(R) + x(G) + x(B)).toUpperCase()
}

function colorOf(n, ctx) {
  if (!n) return null
  const a = n.attrs
  let hex = null
  if (a.rgb) hex = a.rgb.slice(-6).toUpperCase()
  else if (a.theme !== undefined) hex = ctx.theme[+a.theme] || null
  else if (a.indexed !== undefined) {
    const i = +a.indexed
    hex = i >= 64 ? null : ctx.indexed[i] || null
  }
  if (!hex || !/^[0-9A-F]{6}$/.test(hex)) return null
  if (a.tint && +a.tint !== 0) hex = applyTint(hex, +a.tint)
  return '#' + hex
}

/* ---------------- styles ---------------- */
// style -> [px width, css style, rank]
const BORDER = {
  thin: [1, 'solid', 2], hair: [1, 'solid', 1], medium: [2, 'solid', 4], thick: [3, 'solid', 6],
  dashed: [1, 'dashed', 2], dotted: [1, 'dotted', 2], double: [3, 'double', 5],
  mediumDashed: [2, 'dashed', 4], dashDot: [1, 'dashed', 2], mediumDashDot: [2, 'dashed', 4],
  dashDotDot: [1, 'dashed', 2], mediumDashDotDot: [2, 'dashed', 4], slantDashDot: [2, 'dashed', 4],
}

function parseTheme(files) {
  const th = [...DEFAULT_THEME]
  const b = files['xl/theme/theme1.xml']
  if (!b) return th
  try {
    const root = parseXml(bytesToText(b))
    const scheme = kid(kid(root, 'themeElements'), 'clrScheme')
    const order = ['dk1', 'lt1', 'dk2', 'lt2', 'accent1', 'accent2', 'accent3', 'accent4', 'accent5', 'accent6']
    const vals = order.map((k) => {
      const e = kid(scheme, k)
      const c = e && e.children[0]
      if (!c) return null
      return (c.attrs.lastClr || c.attrs.val || '').toUpperCase() || null
    })
    // theme index order: 0=lt1 1=dk1 2=lt2 3=dk2 4..9 accents
    const map = [vals[1], vals[0], vals[3], vals[2], vals[4], vals[5], vals[6], vals[7], vals[8], vals[9]]
    map.forEach((v, i) => { if (v && /^[0-9A-F]{6}$/.test(v)) th[i] = v })
  } catch {}
  return th
}

function flag(n) {
  return !!n && n.attrs.val !== '0' && n.attrs.val !== 'false'
}

function parseStyles(files) {
  const ctx = { theme: parseTheme(files), indexed: [...INDEXED] }
  const b = files['xl/styles.xml']
  const out = { xfs: [], numFmts: {} }
  if (!b) { out.xfs.push(makeXf({}, null, null, null, out)); return out }
  const root = parseXml(bytesToText(b))
  const ic = kid(kid(root, 'colors'), 'indexedColors')
  if (ic) kids(ic, 'rgbColor').forEach((c, i) => { if (c.attrs.rgb) ctx.indexed[i] = c.attrs.rgb.slice(-6).toUpperCase() })
  for (const nf of kids(kid(root, 'numFmts'), 'numFmt')) out.numFmts[nf.attrs.numFmtId] = nf.attrs.formatCode
  const fonts = kids(kid(root, 'fonts'), 'font').map((f) => ({
    b: flag(kid(f, 'b')),
    i: flag(kid(f, 'i')),
    u: !!kid(f, 'u') && kid(f, 'u').attrs.val !== 'none',
    strike: flag(kid(f, 'strike')),
    sz: kid(f, 'sz') ? parseFloat(kid(f, 'sz').attrs.val) : null,
    name: kid(f, 'name') ? kid(f, 'name').attrs.val : null,
    color: colorOf(kid(f, 'color'), ctx),
  }))
  const fills = kids(kid(root, 'fills'), 'fill').map((f) => {
    const p = kid(f, 'patternFill')
    if (!p || p.attrs.patternType !== 'solid') return null
    return colorOf(kid(p, 'fgColor'), ctx)
  })
  const borders = kids(kid(root, 'borders'), 'border').map((bd) => {
    const side = (names) => {
      for (const nm of names) {
        const e = kid(bd, nm)
        if (e && e.attrs.style && BORDER[e.attrs.style]) {
          const [w, st, rank] = BORDER[e.attrs.style]
          const col = colorOf(kid(e, 'color'), ctx) || '#000000'
          return { css: `${w}px ${st} ${col}`, rank }
        }
      }
      return null
    }
    return { l: side(['left', 'start']), r: side(['right', 'end']), t: side(['top']), b: side(['bottom']) }
  })
  for (const x of kids(kid(root, 'cellXfs'), 'xf')) out.xfs.push(makeXf(x, fonts, fills, borders, out))
  if (!out.xfs.length) out.xfs.push(makeXf({}, null, null, null, out))
  return out
}

function makeXf(x, fonts, fills, borders, st) {
  const a = x.attrs || {}
  const numFmtId = +(a.numFmtId || 0)
  const font = fonts && fonts[+(a.fontId || 0)]
  const bg = fills && fills[+(a.fillId || 0)]
  const bd = (borders && borders[+(a.borderId || 0)]) || { l: null, r: null, t: null, b: null }
  const al = (x.children && kid(x, 'alignment') && kid(x, 'alignment').attrs) || {}
  return {
    numFmtId,
    fmt: st.numFmts[numFmtId] !== undefined ? st.numFmts[numFmtId] : numFmtId,
    font, bg, bd,
    h: al.horizontal || 'general',
    v: al.vertical || 'bottom',
    wrap: al.wrapText === '1' || al.wrapText === 'true',
    indent: +(al.indent || 0),
    visible: !!bg || !!(bd.l || bd.r || bd.t || bd.b),
  }
}

/* ---------------- shared strings ---------------- */
function nodeText(n) {
  let s = ''
  for (const c of n.children) {
    if (c.tag === 't') s += decodeXlsxEsc(c.text)
    else if (c.tag === 'r') s += nodeText(c)
    // rPh (phonetic) skipped
  }
  return s
}
function parseSST(files) {
  const b = files['xl/sharedStrings.xml']
  if (!b) return []
  return kids(parseXml(bytesToText(b)), 'si').map(nodeText)
}

/* ---------------- helpers ---------------- */
export function colIndex(letters) {
  let n = 0
  for (const ch of letters.toUpperCase()) n = n * 26 + (ch.charCodeAt(0) - 64)
  return n - 1
}
export function colName(i) {
  let s = ''
  for (let n = i + 1; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + ((n - 1) % 26)) + s
  return s
}
function parseRef(ref) {
  const m = /^\$?([A-Za-z]+)\$?(\d+)$/.exec(ref || '')
  return m ? { r: +m[2] - 1, c: colIndex(m[1]) } : null
}

function formatNumber(v, fmt, date1904) {
  try {
    if (fmt === 49 || fmt === '@') return String(v)
    if (fmt === 0 || fmt === 'General') return String(XLSX.SSF.format('General', v, { date1904 }))
    return String(XLSX.SSF.format(fmt, v, { date1904 }))
  } catch {
    return String(v)
  }
}

/* ---------------- sheet ---------------- */
function buildSheet(info, xml, ctx) {
  const ws = parseXml(xml)
  const fmtPr = kid(ws, 'sheetFormatPr')
  const defRowPx = fmtPr && fmtPr.attrs.defaultRowHeight ? Math.round(parseFloat(fmtPr.attrs.defaultRowHeight) * 4 / 3) : 20
  let defColPx = 64
  if (fmtPr && fmtPr.attrs.defaultColWidth) defColPx = Math.round(parseFloat(fmtPr.attrs.defaultColWidth) * 7)
  else if (fmtPr && fmtPr.attrs.baseColWidth) defColPx = Math.round((parseFloat(fmtPr.attrs.baseColWidth) * 7 + 5) / 8) * 8

  const recs = []
  const rowH = new Map()
  let truncated = false
  let maxR = -1, maxC = -1
  let rIdx = -1
  for (const row of kids(kid(ws, 'sheetData'), 'row')) {
    rIdx = row.attrs.r ? +row.attrs.r - 1 : rIdx + 1
    if (rIdx < MAX_ROWS) {
      if (row.attrs.hidden === '1' || row.attrs.hidden === 'true') rowH.set(rIdx, 0)
      else if (row.attrs.ht) rowH.set(rIdx, Math.round(parseFloat(row.attrs.ht) * 4 / 3))
    }
    let cIdx = -1
    for (const c of kids(row, 'c')) {
      const pr = c.attrs.r ? parseRef(c.attrs.r) : null
      cIdx = pr ? pr.c : cIdx + 1
      const xf = ctx.styles.xfs[+(c.attrs.s || 0)] || ctx.styles.xfs[0]
      const t = c.attrs.t
      const vN = kid(c, 'v'), fN = kid(c, 'f')
      let kind = 'blank', text = '', raw = ''
      if (t === 'inlineStr') {
        text = nodeText(kid(c, 'is') || { children: [] }); raw = text; kind = text === '' ? 'blank' : 's'
      } else if (vN && vN.text !== '') {
        const v = vN.text
        if (t === 's') { text = ctx.sst[+v] ?? ''; raw = text; kind = 's' }
        else if (t === 'str' || t === 'd') { text = decodeXlsxEsc(v); raw = text; kind = 's' }
        else if (t === 'b') { text = v === '1' ? 'TRUE' : 'FALSE'; raw = text; kind = 'b' }
        else if (t === 'e') { text = v; raw = v; kind = 'e' }
        else {
          const num = parseFloat(v)
          if (Number.isFinite(num)) { raw = num; text = formatNumber(num, xf.fmt, ctx.date1904); kind = 'n' }
          else { text = v; raw = v; kind = 's' }
        }
      }
      if (kind === 'blank' && !fN && !xf.visible) continue
      if (rIdx >= MAX_ROWS || cIdx >= MAX_COLS) { truncated = true; continue }
      const formula = fN ? (fN.text || '(shared)') : null
      if (fN) kind = 'f'
      recs.push({ r: rIdx, c: cIdx, xf, kind, text, raw, formula })
      if (text !== '' || xf.visible) { if (rIdx > maxR) maxR = rIdx; if (cIdx > maxC) maxC = cIdx }
    }
  }

  // merges
  let merges = []
  for (const mc of kids(kid(ws, 'mergeCells'), 'mergeCell')) {
    const [a, b] = String(mc.attrs.ref || '').split(':')
    const p1 = parseRef(a), p2 = parseRef(b || a)
    if (!p1 || !p2) continue
    if (p1.r >= MAX_ROWS || p1.c >= MAX_COLS) { truncated = true; continue }
    const r2 = Math.min(p2.r, MAX_ROWS - 1), c2 = Math.min(p2.c, MAX_COLS - 1)
    if (p2.r > r2 || p2.c > c2) truncated = true
    merges.push({ r: p1.r, c: p1.c, rs: r2 - p1.r + 1, cs: c2 - p1.c + 1 })
    if (r2 > maxR) maxR = r2
    if (c2 > maxC) maxC = c2
  }
  const nRows = Math.max(1, maxR + 1), nCols = Math.max(1, maxC + 1)

  // col widths
  const colWidths = Array(nCols).fill(defColPx)
  for (const col of kids(kid(ws, 'cols'), 'col')) {
    const mn = +col.attrs.min - 1, mx = Math.min(+col.attrs.max - 1, nCols - 1)
    const hidden = col.attrs.hidden === '1' || col.attrs.hidden === 'true'
    const w = hidden ? 0 : col.attrs.width ? Math.round(parseFloat(col.attrs.width) * 7) : defColPx
    for (let c = Math.max(0, mn); c <= mx; c++) colWidths[c] = w
  }
  const rowHeights = Array.from({ length: nRows }, (_, r) => (rowH.has(r) ? rowH.get(r) : null))

  // grid
  const recAt = Array.from({ length: nRows }, () => Array(nCols).fill(null))
  for (const x of recs) if (x.r < nRows && x.c < nCols) recAt[x.r][x.c] = x
  merges = merges.filter((m) => m.r < nRows && m.c < nCols)
  const anchor = new Map(), away = new Set()
  for (const m of merges) {
    anchor.set(m.r * 1000 + m.c, m)
    for (let r = m.r; r < m.r + m.rs; r++) for (let c = m.c; c < m.c + m.cs; c++) if (r !== m.r || c !== m.c) away.add(r * 1000 + c)
  }
  const bdOf = (r, c) => (recAt[r] && recAt[r][c] ? recAt[r][c].xf.bd : null)
  // consistent edges: heavier of the two adjacent explicit borders, else gridline
  const best = (a, b) => (a && b ? (b.rank > a.rank ? b : a) : a || b || null)
  const hEdge = (r, c) => best(r > 0 ? bdOf(r - 1, c) && bdOf(r - 1, c).b : null, r < nRows ? bdOf(r, c) && bdOf(r, c).t : null)
  const vEdge = (r, c) => best(c > 0 ? bdOf(r, c - 1) && bdOf(r, c - 1).r : null, c < nCols ? bdOf(r, c) && bdOf(r, c).l : null)
  const css1 = (e) => (e ? e.css : `1px solid ${GRID_COLOR}`)

  const cells = Array.from({ length: nRows }, () => Array(nCols).fill(null))
  for (let r = 0; r < nRows; r++) {
    for (let c = 0; c < nCols; c++) {
      const k = r * 1000 + c
      if (away.has(k)) {
        cells[r][c] = { skip: true, text: '', raw: '', kind: 'blank', formula: null, css: '', editable: false }
        continue
      }
      const m = anchor.get(k)
      const rs = m ? m.rs : 1, cs = m ? m.cs : 1
      let T = null, B = null, L = null, R = null
      for (let i = 0; i < cs; i++) { T = best(T, hEdge(r, c + i)); B = best(B, hEdge(r + rs, c + i)) }
      for (let i = 0; i < rs; i++) { L = best(L, vEdge(r + i, c)); R = best(R, vEdge(r + i, c + cs)) }
      const rec = recAt[r][c]
      if (!rec && !m && !T && !B && !L && !R) continue
      const xf = rec ? rec.xf : ctx.styles.xfs[0]
      const kind = rec ? rec.kind : 'blank'
      const p = []
      if (xf.bg) p.push(`background:${xf.bg}`)
      const f = xf.font
      if (f) {
        if (f.color) p.push(`color:${f.color}`)
        if (f.b) p.push('font-weight:700')
        if (f.i) p.push('font-style:italic')
        const dec = [f.u ? 'underline' : '', f.strike ? 'line-through' : ''].filter(Boolean).join(' ')
        if (dec) p.push(`text-decoration:${dec}`)
        if (f.sz) p.push(`font-size:${f.sz}pt`)
        if (f.name) p.push(`font-family:'${f.name.replace(/['"\\;]/g, '')}',Tahoma,Sarabun,sans-serif`)
      }
      let ta = xf.h
      if (ta === 'general') ta = kind === 'n' ? 'right' : kind === 'b' || kind === 'e' ? 'center' : 'left'
      else if (ta === 'centerContinuous') ta = 'center'
      else if (ta === 'fill' || ta === 'distributed' || ta === 'justify') ta = ta === 'justify' ? 'justify' : 'left'
      p.push(`text-align:${ta}`)
      p.push(`vertical-align:${xf.v === 'center' ? 'middle' : xf.v === 'top' ? 'top' : 'bottom'}`)
      if (xf.wrap) p.push('white-space:pre-wrap', 'word-break:break-word')
      else if (ta === 'left' && !m && kind !== 'n') p.push('white-space:nowrap', 'overflow:visible')
      else p.push('white-space:nowrap')
      if (xf.indent) p.push(ta === 'right' ? `padding-right:${xf.indent * 9 + 2}px` : `padding-left:${xf.indent * 9 + 2}px`)
      p.push(`border-top:${css1(T)}`, `border-right:${css1(R)}`, `border-bottom:${css1(B)}`, `border-left:${css1(L)}`)
      const cell = {
        text: rec ? rec.text : '',
        raw: rec ? rec.raw : '',
        kind,
        formula: rec ? rec.formula : null,
        css: p.join(';'),
        editable: kind === 'n' || kind === 's' || kind === 'blank',
      }
      if (m) { cell.rowspan = m.rs; cell.colspan = m.cs }
      cells[r][c] = cell
    }
  }

  return {
    name: info.name, hidden: info.hidden, path: info.path,
    nRows, nCols, colWidths, rowHeights, merges, cells,
    defaultRowHeight: defRowPx, truncated,
  }
}

export function loadXlsxModel(bytes) {
  const files = unzipSync(bytes, { filter: (f) => !/\.(png|jpe?g|gif|emf|wmf|bin|tiff?|bmp)$/i.test(f.name) })
  const { sheets, date1904 } = resolveSheets(files)
  const ctx = { styles: parseStyles(files), sst: parseSST(files), date1904 }
  const out = []
  for (const s of sheets) {
    const b = files[s.path]
    if (!b) continue
    out.push(buildSheet(s, bytesToText(b), ctx))
  }
  return { sheets: out }
}
