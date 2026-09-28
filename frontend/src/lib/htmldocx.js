// แปลง HTML (จาก rich text editor) เป็นไฟล์ .docx แบบ minimal WordprocessingML
// รองรับ: p, h1-h3, b/strong, i/em, u, s, ul/ol/li, table, br
import { zipSync, strToU8 } from 'fflate'

const esc = (s) =>
  (s || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')

const FONT = 'TH Sarabun New'
function rPr(fmt) {
  let p = `<w:rFonts w:ascii="${FONT}" w:hAnsi="${FONT}" w:cs="${FONT}"/>`
  if (fmt.b) p += '<w:b/><w:bCs/>'
  if (fmt.i) p += '<w:i/><w:iCs/>'
  if (fmt.u) p += '<w:u w:val="single"/>'
  if (fmt.s) p += '<w:strike/>'
  if (fmt.sz) p += `<w:sz w:val="${fmt.sz}"/><w:szCs w:val="${fmt.sz}"/>`
  return `<w:rPr>${p}</w:rPr>`
}
const run = (text, fmt) =>
  `<w:r>${rPr(fmt)}<w:t xml:space="preserve">${esc(text)}</w:t></w:r>`

// เดิน inline nodes -> runs
function inlineRuns(node, fmt) {
  let out = ''
  node.childNodes.forEach((n) => {
    if (n.nodeType === 3) { // text
      if (n.textContent) out += run(n.textContent, fmt)
    } else if (n.nodeType === 1) {
      const t = n.tagName.toLowerCase()
      if (t === 'br') { out += '<w:r><w:br/></w:r>'; return }
      const f = { ...fmt }
      if (t === 'b' || t === 'strong') f.b = true
      if (t === 'i' || t === 'em') f.i = true
      if (t === 'u') f.u = true
      if (t === 's' || t === 'strike' || t === 'del') f.s = true
      out += inlineRuns(n, f)
    }
  })
  return out
}

const para = (runsXml, opts = {}) => {
  let pPr = ''
  const props = []
  if (opts.indent) props.push(`<w:ind w:left="${opts.indent}"/>`)
  if (props.length) pPr = `<w:pPr>${props.join('')}</w:pPr>`
  return `<w:p>${pPr}${runsXml || run('', {})}</w:p>`
}

function cellXml(node, fmt) {
  return `<w:tc><w:tcPr><w:tcW w:w="0" w:type="auto"/></w:tcPr>${para(inlineRuns(node, fmt))}</w:tc>`
}

function tableXml(tbl) {
  let rows = ''
  tbl.querySelectorAll('tr').forEach((tr) => {
    let cells = ''
    tr.querySelectorAll('th,td').forEach((td) => {
      const isTh = td.tagName.toLowerCase() === 'th'
      cells += cellXml(td, isTh ? { b: true } : {})
    })
    if (cells) rows += `<w:tr>${cells}</w:tr>`
  })
  const borders = ['top', 'left', 'bottom', 'right', 'insideH', 'insideV']
    .map((s) => `<w:${s} w:val="single" w:sz="4" w:space="0" w:color="999999"/>`).join('')
  return `<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblBorders>${borders}</w:tblBorders></w:tblPr>${rows}</w:tbl>`
}

function blockXml(node) {
  const t = node.tagName ? node.tagName.toLowerCase() : ''
  if (t === 'h1') return para(inlineRuns(node, { b: true, sz: 36 }))
  if (t === 'h2') return para(inlineRuns(node, { b: true, sz: 32 }))
  if (t === 'h3') return para(inlineRuns(node, { b: true, sz: 28 }))
  if (t === 'table') return tableXml(node)
  if (t === 'ul' || t === 'ol') {
    let out = ''
    let i = 1
    node.querySelectorAll(':scope > li').forEach((li) => {
      const prefix = t === 'ol' ? `${i}.  ` : '•  '
      out += para(run(prefix, {}) + inlineRuns(li, {}), { indent: 360 })
      i++
    })
    return out
  }
  if (t === 'br') return para('')
  // p, div, หรือ inline ที่ห่อ
  return para(inlineRuns(node, {}))
}

export function htmlToDocx(html) {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, 'text/html')
  const body = doc.body
  let content = ''
  body.childNodes.forEach((n) => {
    if (n.nodeType === 3) {
      if (n.textContent.trim()) content += para(run(n.textContent, {}))
    } else if (n.nodeType === 1) {
      content += blockXml(n)
    }
  })
  if (!content) content = para('')

  const documentXml =
    `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
    `<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">` +
    `<w:body>${content}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>`

  const contentTypes =
    `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
    `<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">` +
    `<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>` +
    `<Default Extension="xml" ContentType="application/xml"/>` +
    `<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>` +
    `</Types>`

  const rels =
    `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` +
    `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
    `<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>` +
    `</Relationships>`

  return zipSync({
    '[Content_Types].xml': strToU8(contentTypes),
    '_rels/.rels': strToU8(rels),
    'word/document.xml': strToU8(documentXml),
  })
}
