// Run from frontend/:  node scripts/test-xlsx.mjs
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import * as XLSX from 'xlsx'
import { unzipSync } from 'fflate'
import { loadXlsxModel, resolveSheets } from '../src/lib/xlsxview.js'
import { patchXlsx } from '../src/lib/xlsxpatch.js'

const here = path.dirname(fileURLToPath(import.meta.url))
const roots = [path.resolve(here, '../../data/docs'), path.resolve(here, '../../data/storage')]
const outDir = process.env.TEST_OUT
function walk(d, acc = []) {
  if (!fs.existsSync(d)) return acc
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name)
    if (e.isDirectory()) walk(p, acc)
    else if (/\.xlsx$/i.test(e.name) && !e.name.startsWith('~$')) acc.push(p)
  }
  return acc
}
const files = roots.flatMap((r) => walk(r))
let pass = 0, fail = 0, checks = 0
const fails = []
const THAI = 'ทดสอบแก้ไข <&> "x"'
const eq = (a, b, msg) => {
  checks++
  if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`${msg}: ${JSON.stringify(a)?.slice(0, 120)} != ${JSON.stringify(b)?.slice(0, 120)}`)
}
const ok = (v, msg) => { checks++; if (!v) throw new Error(msg) }

function find(model, pred) {
  for (const s of model.sheets) {
    if (s.hidden) continue
    for (let r = 0; r < s.nRows; r++) for (let c = 0; c < s.nCols; c++) {
      const cell = s.cells[r][c]
      if (cell && !cell.skip && pred(cell, s, r, c)) return { sheet: s, r, c, cell }
    }
  }
  return null
}

function compareModels(a, b, edited, strictCss) {
  eq(a.sheets.length, b.sheets.length, 'sheet count')
  a.sheets.forEach((sa, i) => {
    const sb = b.sheets[i]
    eq(sa.name, sb.name, 'name')
    eq(sa.merges, sb.merges, 'merges ' + sa.name)
    eq(sa.colWidths, sb.colWidths, 'colWidths ' + sa.name)
    eq(sa.rowHeights, sb.rowHeights, 'rowHeights ' + sa.name)
    eq([sa.nRows, sa.nCols], [sb.nRows, sb.nCols], 'dims ' + sa.name)
    for (let r = 0; r < sa.nRows; r++) for (let c = 0; c < sa.nCols; c++) {
      if (edited.has(`${sa.name}|${r}|${c}`)) continue
      const x = sa.cells[r][c], y = sb.cells[r][c]
      if (strictCss) eq(x, y, `cell ${sa.name}!${r},${c}`)
      else eq(x && [x.text, x.raw, x.kind], y && [y.text, y.raw, y.kind], `cell ${sa.name}!${r},${c}`)
    }
  })
}

function compareZips(origBytes, newBytes, allowChanged) {
  const A = unzipSync(origBytes), B = unzipSync(newBytes)
  for (const k of Object.keys(A)) {
    if (allowChanged.has(k)) continue
    ok(B[k], 'missing entry ' + k)
    ok(Buffer.compare(Buffer.from(A[k]), Buffer.from(B[k])) === 0, 'entry changed ' + k)
  }
  eq(Object.keys(B)[0], '[Content_Types].xml', 'content types first')
}

function sheetXmlSplice(origBytes, newBytes, sheetPath) {
  const a = Buffer.from(unzipSync(origBytes)[sheetPath]).toString('utf8')
  const b = Buffer.from(unzipSync(newBytes)[sheetPath]).toString('utf8')
  const strip = (s) => s.replace(/<dimension\b[^>]*>/, '')
  const head = (s) => strip(s.slice(0, s.search(/<sheetData\b/)))
  const tail = (s) => s.slice(s.lastIndexOf('</sheetData>'))
  eq(head(a), head(b), 'sheet head preserved (root ns decls etc)')
  eq(tail(a), tail(b), 'sheet tail preserved')
}

for (const f of files) {
  const rel = path.relative(path.resolve(here, '../..'), f)
  try {
    const orig = new Uint8Array(fs.readFileSync(f))
    const model = loadXlsxModel(orig)
    ok(model.sheets.length > 0, 'no sheets')
    const wbInfo = resolveSheets(unzipSync(orig))
    const dumpCols = model.sheets.map((s) => s.nCols * s.nRows)
    ok(dumpCols.every((n) => n >= 1), 'dims')

    // ---- A) Thai text + numeric edit
    const edits = []
    const edited = new Set()
    const t = find(model, (c) => c.kind === 's' && c.editable)
    if (t) {
      edits.push({ sheet: t.sheet.name, r: t.r, c: t.c, value: THAI })
      edited.add(`${t.sheet.name}|${t.r}|${t.c}`)
    }
    const n = find(model, (c, s, r, c2) => c.kind === 'n' && c.editable && !(t && s === t.sheet && r === t.r && c2 === t.c))
    let numTarget = n
    if (n) {
      edits.push({ sheet: n.sheet.name, r: n.r, c: n.c, value: '12345.67' })
      edited.add(`${n.sheet.name}|${n.r}|${n.c}`)
    }
    ok(edits.length > 0, 'nothing editable to test')
    const patched = patchXlsx(orig, edits)
    if (outDir) fs.writeFileSync(path.join(outDir, path.basename(f)), patched)
    const m2 = loadXlsxModel(patched)
    if (t) {
      const cell = m2.sheets.find((s) => s.name === t.sheet.name).cells[t.r][t.c]
      eq(cell.text, THAI, 'thai text')
      eq(cell.css, t.cell.css, 'edited cell css preserved')
    }
    if (n) {
      const cell = m2.sheets.find((s) => s.name === n.sheet.name).cells[n.r][n.c]
      eq(cell.raw, 12345.67, 'numeric raw')
      eq(cell.kind, 'n', 'numeric kind')
      eq(cell.css, n.cell.css, 'num cell css preserved')
    }
    compareModels(model, m2, edited, true)
    const changedParts = new Set(edits.map((e) => wbInfo.sheets.find((s) => s.name === e.sheet).path))
    compareZips(orig, patched, changedParts)
    for (const p of changedParts) sheetXmlSplice(orig, patched, p)
    const wbRead = XLSX.read(patched, { type: 'array' })
    eq(wbRead.SheetNames.length, model.sheets.length, 'SheetJS sheet count')
    if (t) {
      const ws = wbRead.Sheets[t.sheet.name]
      eq(ws[XLSX.utils.encode_cell({ r: t.r, c: t.c })].v, THAI, 'SheetJS reads thai')
    }

    // ---- B) clear a cell (empty value keeps style)
    if (t) {
      const p2 = patchXlsx(orig, [{ sheet: t.sheet.name, r: t.r, c: t.c, value: '' }])
      const m3 = loadXlsxModel(p2)
      const cell = m3.sheets.find((s) => s.name === t.sheet.name).cells[t.r][t.c]
      eq(cell ? cell.text : '', '', 'cleared text')
      const bd = (x) => (x ? x.css.split(';').filter((q) => /^(border|background)/.test(q)) : [])
      if (cell) eq(bd(cell), bd(t.cell), 'cleared border/bg kept')
    }

    // ---- C) formula drop
    const fcell = find(model, (c) => c.kind === 'f')
    if (fcell) {
      const p3 = patchXlsx(orig, [{ sheet: fcell.sheet.name, r: fcell.r, c: fcell.c, value: 'ทดสอบสูตร' }])
      const z = unzipSync(p3)
      const m3 = loadXlsxModel(p3)
      const cell = m3.sheets.find((s) => s.name === fcell.sheet.name).cells[fcell.r][fcell.c]
      if (cell.text === 'ทดสอบสูตร') {
        ok(!z['xl/calcChain.xml'], 'calcChain removed')
        ok(/fullCalcOnLoad="1"/.test(Buffer.from(z['xl/workbook.xml']).toString()), 'fullCalcOnLoad')
        ok(!/calcChain/.test(Buffer.from(z['[Content_Types].xml']).toString()), 'ct calcChain')
        ok(!/calcChain/.test(Buffer.from(z['xl/_rels/workbook.xml.rels']).toString()), 'rels calcChain')
        eq(cell.formula, null, 'formula dropped')
      }
      XLSX.read(p3, { type: 'array' })
    }

    // ---- D) create cells: new row after the end, and far column in first row
    const s0 = model.sheets.find((s) => !s.hidden && s.nRows + 3 < 300 && s.nCols + 2 < 60) || model.sheets.find((s) => !s.hidden)
    const capped = !(s0.nRows + 3 < 300 && s0.nCols + 2 < 60)
    const newEdits = [
      { sheet: s0.name, r: s0.nRows + 1, c: 1, value: 'แถวใหม่' },
      { sheet: s0.name, r: s0.nRows + 2, c: 0, value: '42' },
      { sheet: s0.name, r: 0, c: s0.nCols + 1, value: 'คอลัมน์ใหม่' },
    ]
    if (capped) newEdits.length = 0
    const p4 = patchXlsx(orig, newEdits)
    const m4 = loadXlsxModel(p4)
    const s4 = m4.sheets.find((s) => s.name === s0.name)
    if (!capped) {
    eq(s4.cells[s0.nRows + 1][1].text, 'แถวใหม่', 'new row text')
    eq(String(s4.cells[s0.nRows + 2][0].text), '42', 'new row number')
    eq(s4.cells[0][s0.nCols + 1].text, 'คอลัมน์ใหม่', 'new col text')
    const wb4 = XLSX.read(p4, { type: 'array' })
    const ref = wb4.Sheets[s0.name]['!ref']
    const rg = XLSX.utils.decode_range(ref)
    if (!capped) ok(rg.e.r >= s0.nRows + 2 && rg.e.c >= s0.nCols + 1, 'dimension/ref extended ' + ref)
    }
    compareZips(orig, p4, new Set([s0.path]))

    pass++
    console.log('PASS', rel, `(${model.sheets.length} sheets${fcell ? ', formulas' : ''})`)
  } catch (e) {
    fail++
    fails.push(rel)
    console.log('FAIL', rel, '->', e.message, process.env.STACK ? e.stack : '')
  }
}
console.log(`\n${pass} passed, ${fail} failed of ${files.length} files; ${checks} assertions`)
process.exit(fail ? 1 : 0)
