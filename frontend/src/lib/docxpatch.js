// แก้ไฟล์ .docx ต้นฉบับ "ในที่เดิม" จาก HTML ที่ผู้ใช้แก้บนเว็บ
// แทนการสร้างไฟล์ใหม่ทั้งฉบับ (ซึ่งทำให้หัวกระดาษ/โลโก้/ตาราง/ฟอนต์หาย) — แตะเฉพาะย่อหน้า/run ที่เปลี่ยนจริง
// ที่เหลือใน package (styles, header/footer, รูป, numbering ฯลฯ) คงไบต์เดิมทั้งหมด
//
// สัญญากับ converter/docx.go:
//   div.p[data-p="part:idx"]   ย่อหน้า (idx = ลำดับ w:p ใน part นั้น แบบ pre-order)
//   tr[data-tr="part:idx"]     แถวตาราง
//   span[data-r="p.k"]         run ข้อความ k ของย่อหน้า p   (ดู classifyRun / collectRuns ด้านล่าง ต้องตรงกับฝั่ง Go)
//   span.atom[data-keep][data-a="p.a"]  เนื้อหาที่แก้ไม่ได้ (รูป สัญลักษณ์ ตัวแบ่งหน้า) — คงไว้ตามเดิม
import { unzipSync, zipSync, strFromU8, strToU8 } from 'fflate'

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
const XMLNS = 'http://www.w3.org/XML/1998/namespace'

// ---------- โครงสร้าง run (ต้องตรงกับ converter/docx.go) ----------
const CONTAINERS = new Set(['hyperlink', 'fldSimple', 'smartTag', 'ins', 'moveTo', 'customXml', 'sdt', 'sdtContent', 'dir', 'bdo'])

function classifyRun(r) {
  let hasText = false
  let hasAtom = false
  for (const k of r.children) {
    if (k.namespaceURI !== W) {
      if (k.localName === 'AlternateContent') hasAtom = true
      continue
    }
    switch (k.localName) {
      case 't': case 'tab': case 'cr':
        hasText = true
        break
      case 'br': {
        const t = k.getAttributeNS(W, 'type')
        if (t === 'page' || t === 'column') hasAtom = true
        else hasText = true
        break
      }
      case 'drawing': case 'pict': case 'object': case 'sym': case 'noBreakHyphen': case 'softHyphen': case 'ptab':
        hasAtom = true
        break
    }
  }
  return hasAtom ? 2 : hasText ? 1 : 0
}

function collectRuns(p) {
  const out = []
  const walk = (n) => {
    for (const k of n.children) {
      if (k.namespaceURI !== W) continue
      if (k.localName === 'r') {
        const c = classifyRun(k)
        if (c) out.push({ el: k, kind: c })
      } else if (CONTAINERS.has(k.localName)) walk(k)
    }
  }
  walk(p)
  return out
}

const wChildren = (el, name) => [...el.children].filter((k) => k.namespaceURI === W && k.localName === name)
const wChild = (el, name) => wChildren(el, name)[0] || null

// ---------- อ่านข้อความ/รูปแบบจาก DOM ที่กำลังแก้ ----------
function flagsOf(el, stop) {
  const cs = getComputedStyle(el)
  let u = false
  let s = false
  for (let e = el; e && e !== stop; e = e.parentElement) {
    const d = getComputedStyle(e).textDecorationLine || ''
    if (d.includes('underline')) u = true
    if (d.includes('line-through')) s = true
  }
  return { b: parseInt(cs.fontWeight, 10) >= 600, i: cs.fontStyle === 'italic' || cs.fontStyle === 'oblique', u, s }
}

function isTrailing(el, stop) {
  for (let e = el; e && e !== stop; e = e.parentNode) {
    if (e.nextSibling) return false
  }
  return true
}

export function extractPara(div) {
  const frags = []
  const atoms = []
  const push = (text, el, rk) => {
    if (!text) return
    const f = flagsOf(el, div.parentElement)
    const l = frags[frags.length - 1]
    if (l && l.rk === rk && l.b === f.b && l.i === f.i && l.u === f.u && l.s === f.s) l.text += text
    else frags.push({ rk, text, b: f.b, i: f.i, u: f.u, s: f.s })
  }
  const walk = (node, rk) => {
    for (const ch of node.childNodes) {
      if (ch.nodeType === 3) {
        push(ch.data.replace(/​/g, ''), node, rk)
      } else if (ch.nodeType === 1) {
        if (ch.classList.contains('txbx') || ch.classList.contains('num')) continue
        if (ch.hasAttribute('data-keep')) {
          atoms.push(ch.getAttribute('data-a'))
          continue
        }
        if (ch.hasAttribute('data-tab')) {
          push('\t', ch, rk)
          continue
        }
        if (ch.tagName === 'BR') {
          if (ch.hasAttribute('data-b') || !isTrailing(ch, div)) push('\n', node, rk)
          continue
        }
        walk(ch, ch.getAttribute('data-r') || rk)
      }
    }
  }
  walk(div, null)
  return { frags, atoms }
}

const fragSig = (frags) => JSON.stringify(frags.map((f) => [f.rk, f.text, f.b, f.i, f.u, f.s]))

/** เก็บสภาพเริ่มต้นของ editor ไว้เทียบตอนบันทึก */
export function makeBaseline(root) {
  const paras = new Map()
  const runFlags = new Map()
  const runSig = new Map()
  for (const div of root.querySelectorAll('[data-p]')) {
    const key = div.getAttribute('data-p')
    const { frags, atoms } = extractPara(div)
    paras.set(key, { sig: fragSig(frags), atoms })
    const byRun = new Map()
    for (const f of frags) {
      if (!f.rk) continue
      if (!runFlags.has(f.rk)) runFlags.set(f.rk, { b: f.b, i: f.i, u: f.u, s: f.s })
      if (!byRun.has(f.rk)) byRun.set(f.rk, [])
      byRun.get(f.rk).push(f)
    }
    for (const [rk, list] of byRun) runSig.set(rk, fragSig(list))
  }
  const trs = new Set([...root.querySelectorAll('tr[data-tr]')].map((t) => t.getAttribute('data-tr')))
  return { paras, runFlags, runSig, trs }
}

// ---------- ช่วยจัดการ XML ----------
const RPR_ORDER = ['rStyle', 'rFonts', 'b', 'bCs', 'i', 'iCs', 'caps', 'smallCaps', 'strike', 'dstrike', 'outline', 'shadow', 'emboss',
  'imprint', 'noProof', 'snapToGrid', 'vanish', 'webHidden', 'color', 'spacing', 'w', 'kern', 'position', 'sz', 'szCs', 'highlight', 'u',
  'effect', 'bdr', 'shd', 'fitText', 'vertAlign', 'rtl', 'cs', 'em', 'lang', 'eastAsianLayout', 'specVanish', 'oMath']

function mk(doc, name) {
  return doc.createElementNS(W, 'w:' + name)
}

function setProp(rPr, name, on) {
  const doc = rPr.ownerDocument
  for (const old of wChildren(rPr, name)) rPr.removeChild(old)
  const el = mk(doc, name)
  if (name === 'u') el.setAttributeNS(W, 'w:val', on ? 'single' : 'none')
  else if (!on) el.setAttributeNS(W, 'w:val', '0')
  const myOrder = RPR_ORDER.indexOf(name)
  let before = null
  for (const k of rPr.children) {
    if (k.namespaceURI !== W) continue
    if (RPR_ORDER.indexOf(k.localName) > myOrder) {
      before = k
      break
    }
  }
  rPr.insertBefore(el, before)
}

function getRPr(run) {
  let rPr = wChild(run, 'rPr')
  if (!rPr) {
    rPr = mk(run.ownerDocument, 'rPr')
    run.insertBefore(rPr, run.firstChild)
  }
  return rPr
}

function appendText(run, text) {
  const doc = run.ownerDocument
  for (const piece of text.split(/(\t|\n)/)) {
    if (piece === '') continue
    if (piece === '\t') run.appendChild(mk(doc, 'tab'))
    else if (piece === '\n') run.appendChild(mk(doc, 'br'))
    else {
      const t = mk(doc, 't')
      t.setAttributeNS(XMLNS, 'xml:space', 'preserve')
      t.textContent = piece
      run.appendChild(t)
    }
  }
}

function buildRun(doc, tmpl, text, flags, base) {
  const r = tmpl ? tmpl.cloneNode(true) : mk(doc, 'r')
  for (const k of [...r.childNodes]) {
    if (!(k.nodeType === 1 && k.namespaceURI === W && k.localName === 'rPr')) r.removeChild(k)
  }
  if (flags && base) {
    const rPr = () => getRPr(r)
    if (flags.b !== base.b) { setProp(rPr(), 'b', flags.b); setProp(rPr(), 'bCs', flags.b) }
    if (flags.i !== base.i) { setProp(rPr(), 'i', flags.i); setProp(rPr(), 'iCs', flags.i) }
    if (flags.u !== base.u) setProp(rPr(), 'u', flags.u)
    if (flags.s !== base.s) setProp(rPr(), 'strike', flags.s)
  }
  appendText(r, text)
  return r
}

const NONE = { b: false, i: false, u: false, s: false }

// ---------- package ----------
class Pkg {
  constructor(bytes) {
    this.files = unzipSync(bytes)
    this.parts = new Map()
    this.dirty = new Set()
  }
  touch(el) {
    this.dirty.add(el.ownerDocument)
  }
  part(name) {
    if (!this.parts.has(name)) {
      const raw = this.files[`word/${name}.xml`]
      if (!raw) {
        this.parts.set(name, null)
      } else {
        const doc = new DOMParser().parseFromString(strFromU8(raw), 'application/xml')
        if (doc.getElementsByTagName('parsererror').length) throw new Error(`อ่านโครงสร้าง ${name}.xml ไม่ได้`)
        this.parts.set(name, {
          name,
          doc,
          ps: [...doc.getElementsByTagNameNS(W, 'p')],
          trs: [...doc.getElementsByTagNameNS(W, 'tr')],
          runs: new Map(),
        })
      }
    }
    return this.parts.get(name)
  }
  p(key) {
    const [name, idx] = splitKey(key)
    return this.part(name)?.ps[idx] || null
  }
  tr(key) {
    const [name, idx] = splitKey(key)
    return this.part(name)?.trs[idx] || null
  }
  // run ข้อความตาม key "p.k" ใน part ที่ระบุ
  run(partName, rk) {
    const part = this.part(partName)
    if (!part || !rk) return null
    const [pi, k] = rk.split('.').map(Number)
    if (!part.runs.has(pi)) {
      const p = part.ps[pi]
      part.runs.set(pi, p ? collectRuns(p).filter((x) => x.kind === 1).map((x) => x.el) : [])
    }
    return part.runs.get(pi)[k] || null
  }
  finish() {
    for (const part of this.parts.values()) {
      if (!part || !this.dirty.has(part.doc)) continue
      const xml = new XMLSerializer().serializeToString(part.doc)
      const body = xml.replace(/^<\?xml[^>]*\?>\s*/, '')
      this.files[`word/${part.name}.xml`] = strToU8('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n' + body)
    }
    return zipSync(this.files)
  }
}

function splitKey(key) {
  const i = key.lastIndexOf(':')
  return [key.slice(0, i), parseInt(key.slice(i + 1), 10)]
}

function plainText(div) {
  return extractPara(div).frags.map((f) => f.text).join('')
}

// เขียนย่อหน้าให้เหลือข้อความล้วน (ใช้กับแถวใหม่/ล้างข้อความ)
function setParaPlain(p, text) {
  const items = collectRuns(p)
  const firstText = items.find((x) => x.kind === 1)
  const tmpl = firstText ? firstText.el : null
  for (const it of items) it.el.parentNode.removeChild(it.el)
  if (text) p.appendChild(buildRun(p.ownerDocument, tmpl, text))
}

function blankTextRuns(p) {
  for (const it of collectRuns(p)) if (it.kind === 1) it.el.parentNode.removeChild(it.el)
}

function shellOf(p) {
  const n = p.cloneNode(true)
  for (const k of [...n.childNodes]) {
    if (!(k.nodeType === 1 && k.namespaceURI === W && k.localName === 'pPr')) n.removeChild(k)
  }
  const pPr = wChild(n, 'pPr')
  if (pPr) for (const s of wChildren(pPr, 'sectPr')) pPr.removeChild(s)
  return n
}

// ---------- แก้ย่อหน้าที่มีอยู่ ----------
function editPara(pkg, partName, p, pidx, frags, atoms, b, base) {
  const doc = p.ownerDocument
  const items = collectRuns(p)
  const textRuns = new Map()
  const atomRuns = new Map()
  let k = 0
  let a = 0
  for (const it of items) {
    if (it.kind === 1) textRuns.set(`${pidx}.${k++}`, it.el)
    else atomRuns.set(`${pidx}.${a++}`, it.el)
  }
  for (const id of b.atoms) {
    if (!atoms.includes(id)) atomRuns.get(id)?.parentNode?.removeChild(atomRuns.get(id))
  }

  // กำหนด run เจ้าของของแต่ละชิ้นข้อความ (ข้อความที่พิมพ์ใหม่ไปอยู่กับ run ข้างเคียง)
  const host = frags.map((f) => (f.rk && textRuns.has(f.rk) ? f.rk : null))
  for (let i = 1; i < host.length; i++) if (!host[i]) host[i] = host[i - 1]
  for (let i = host.length - 2; i >= 0; i--) if (!host[i]) host[i] = host[i + 1]

  const tmplFor = (f, h) => pkg.run(partName, f.rk) || (h && textRuns.get(h)) || null
  const flagsFor = (f, h) => base.runFlags.get(f.rk) || (h && base.runFlags.get(h)) || NONE
  const mkRuns = (list) =>
    list.map(({ f, h }) => buildRun(doc, tmplFor(f, h), f.text, f, flagsFor(f, h)))

  for (const [rk, run] of textRuns) {
    const mine = frags.map((f, i) => ({ f, h: host[i] })).filter((x) => x.h === rk)
    if (fragSig(mine.map((x) => x.f)) === (base.runSig.get(rk) || '[]')) continue
    const parent = run.parentNode
    for (const nr of mkRuns(mine)) parent.insertBefore(nr, run)
    parent.removeChild(run)
  }
  // ไม่มี run ข้อความเดิมให้ยึด -> ต่อท้ายย่อหน้า
  const orphans = frags.map((f, i) => ({ f, h: host[i] })).filter((x) => !x.h)
  for (const nr of mkRuns(orphans)) p.appendChild(nr)
}

// ---------- ตัวหลัก ----------
export function patchDocx(origBytes, root, base) {
  const pkg = new Pkg(origBytes)

  // 1) แถวตาราง: ลบ / เพิ่ม
  const domTr = [...root.querySelectorAll('tr')]
  const live = new Set(domTr.filter((t) => t.hasAttribute('data-tr')).map((t) => t.getAttribute('data-tr')))
  const removedRows = []
  for (const key of base.trs) {
    if (live.has(key)) continue
    const x = pkg.tr(key)
    if (!x) continue
    const siblings = [...x.parentNode.children].filter((c) => c.namespaceURI === W && c.localName === 'tr')
    if (siblings.length <= 1) continue
    removedRows.push(x)
    pkg.touch(x)
    x.parentNode.removeChild(x)
  }
  const xmlOfTr = new Map()
  for (const tr of domTr) {
    if (tr.hasAttribute('data-tr')) {
      xmlOfTr.set(tr, pkg.tr(tr.getAttribute('data-tr')))
      continue
    }
    const prev = tr.previousElementSibling
    const next = tr.nextElementSibling
    let ref = null
    let after = true
    if (prev && xmlOfTr.get(prev)) ref = xmlOfTr.get(prev)
    else if (next && next.hasAttribute('data-tr')) {
      ref = pkg.tr(next.getAttribute('data-tr'))
      after = false
    }
    if (!ref || !ref.parentNode) continue
    const clone = ref.cloneNode(true)
    const tcs = wChildren(clone, 'tc')
    const tds = [...tr.children].filter((c) => c.tagName === 'TD')
    tcs.forEach((tc, i) => {
      const tcPr = wChild(tc, 'tcPr')
      if (tcPr) for (const vm of wChildren(tcPr, 'vMerge')) tcPr.removeChild(vm)
      const ps = wChildren(tc, 'p')
      for (const k of [...tc.children]) if (k !== tcPr && k !== ps[0]) tc.removeChild(k)
      if (!ps[0]) tc.appendChild(mk(tc.ownerDocument, 'p'))
      const p0 = wChild(tc, 'p')
      const td = tds[i]
      const text = td ? [...td.querySelectorAll(':scope > div.p')].map(plainText).join('\n') : ''
      setParaPlain(p0, text)
    })
    pkg.touch(ref)
    if (after) ref.parentNode.insertBefore(clone, ref.nextSibling)
    else ref.parentNode.insertBefore(clone, ref)
    xmlOfTr.set(tr, clone)
  }

  // 2) ย่อหน้า
  const inRemoved = (p) => removedRows.some((r) => r.contains(p))
  const seen = new Set()
  let prevXml = null
  let prevCont = null
  for (const div of root.querySelectorAll('div.p')) {
    if (div.closest('tr:not([data-tr])')) continue
    const key = div.getAttribute('data-p')
    const cont = div.parentElement?.closest('td, .txbx, .docx-hdr, .docx-ftr, .docx-body') || null
    const { frags, atoms } = extractPara(div)
    const isNew = !key || seen.has(key) || !base.paras.has(key)
    if (!isNew) {
      seen.add(key)
      const xml = pkg.p(key)
      if (!xml) continue
      const b = base.paras.get(key)
      if (fragSig(frags) !== b.sig || atoms.join() !== b.atoms.join()) {
        const [partName, pidx] = splitKey(key)
        pkg.touch(xml)
        editPara(pkg, partName, xml, pidx, frags, atoms, b, base)
      }
      prevXml = xml
      prevCont = cont
      continue
    }
    // ย่อหน้าใหม่ (กด Enter / วาง)
    const refKey = key && base.paras.has(key) ? key : null
    const refXml = refKey ? pkg.p(refKey) : prevXml
    if (!refXml) continue
    const partName = refKey ? splitKey(refKey)[0] : null
    const np = shellOf(refXml)
    const doc = refXml.ownerDocument
    const firstText = collectRuns(refXml).find((x) => x.kind === 1)
    for (const f of frags) {
      const tmpl = (partName && pkg.run(partName, f.rk)) || (firstText ? firstText.el : null)
      const bf = base.runFlags.get(f.rk) || (tmpl ? base.runFlags.get(f.rk) : null) || NONE
      np.appendChild(buildRun(doc, tmpl, f.text, f, bf))
    }
    const anchor = prevXml && prevCont === cont ? prevXml : refXml
    pkg.touch(anchor)
    anchor.parentNode.insertBefore(np, anchor.nextSibling)
    prevXml = np
    prevCont = cont
  }

  // 3) ย่อหน้าที่ถูกลบ
  const stillAtoms = new Set([...root.querySelectorAll('[data-a]')].map((e) => e.getAttribute('data-a').split('.')[0]))
  for (const key of base.paras.keys()) {
    if (seen.has(key)) continue
    const xml = pkg.p(key)
    if (!xml || !xml.parentNode || inRemoved(xml)) continue
    const [, pidx] = splitKey(key)
    const parent = xml.parentNode
    const inCell = parent.namespaceURI === W && parent.localName === 'tc'
    const pPr = wChild(xml, 'pPr')
    const hasSect = pPr && wChild(pPr, 'sectPr')
    const hasAtom = collectRuns(xml).some((x) => x.kind === 2) && stillAtoms.has(String(pidx))
    pkg.touch(xml)
    if (inCell || hasSect || hasAtom) blankTextRuns(xml)
    else parent.removeChild(xml)
  }

  return pkg.finish()
}

// ---------- แถวตารางบน editor ----------
function rowOfSelection(root) {
  const sel = window.getSelection()
  const node = sel && sel.anchorNode
  const el = node && (node.nodeType === 1 ? node : node.parentElement)
  const tr = el?.closest('tr')
  return tr && root.contains(tr) ? tr : null
}

export function insertRowBelow(root) {
  const tr = rowOfSelection(root)
  if (!tr) return 'วางเคอร์เซอร์ในช่องตารางก่อน'
  if ([...tr.children].some((td) => td.rowSpan > 1)) return 'แถวนี้มีเซลล์ที่ผสานแนวตั้ง — เพิ่มแถวไม่ได้'
  const clone = tr.cloneNode(true)
  clone.removeAttribute('data-tr')
  for (const td of clone.children) {
    const ps = [...td.querySelectorAll(':scope > div.p')]
    ps.slice(1).forEach((p) => p.remove())
    for (const e of td.querySelectorAll('[data-p],[data-r],[data-a]')) {
      e.removeAttribute('data-p'); e.removeAttribute('data-r'); e.removeAttribute('data-a')
    }
    const p0 = ps[0]
    if (p0) p0.innerHTML = '<br>'
    else td.innerHTML = '<div class="p"><br></div>'
  }
  tr.after(clone)
  return ''
}

export function deleteRow(root) {
  const tr = rowOfSelection(root)
  if (!tr) return 'วางเคอร์เซอร์ในช่องตารางก่อน'
  if ([...tr.children].some((td) => td.rowSpan > 1)) return 'แถวนี้มีเซลล์ที่ผสานแนวตั้ง — ลบแถวไม่ได้'
  if (tr.parentElement.children.length <= 1) return 'ลบแถวสุดท้ายของตารางไม่ได้'
  tr.remove()
  return ''
}
