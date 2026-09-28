/** บรรจุ/แยกบันทึกประวัติการแก้ไขไฟล์ (เก็บใน versions.note) */

export const SRC_LABEL = { original: 'ต้นฉบับ', edit: 'แก้บนเว็บ', upload: 'อัปโหลด' }

const PREFIX = {
  topic: 'เรื่อง:',
  detail: 'รายละเอียด:',
  pages: 'หน้าที่แก้ไข:',
}

export function packChangeNote({ topic = '', detail = '', pages = '', extra = '' } = {}) {
  const lines = []
  if (topic.trim()) lines.push(`${PREFIX.topic} ${topic.trim()}`)
  if (detail.trim()) lines.push(`${PREFIX.detail} ${detail.trim()}`)
  if (pages.trim()) lines.push(`${PREFIX.pages} ${pages.trim()}`)
  if (extra.trim()) lines.push(extra.trim())
  return lines.join('\n')
}

export function parseChangeNote(note = '') {
  const out = { topic: '', detail: '', pages: '', extra: '', raw: note || '' }
  if (!note) return out
  const extras = []
  for (const line of String(note).split('\n')) {
    const t = line.trim()
    if (t.startsWith(PREFIX.topic)) out.topic = t.slice(PREFIX.topic.length).trim()
    else if (t.startsWith(PREFIX.detail)) out.detail = t.slice(PREFIX.detail.length).trim()
    else if (t.startsWith(PREFIX.pages)) out.pages = t.slice(PREFIX.pages.length).trim()
    else if (t) extras.push(t)
  }
  if (!out.topic && !out.detail && !out.pages && note.trim()) {
    out.topic = note.trim()
  } else {
    out.extra = extras.join('\n')
  }
  return out
}

export function srcLabel(source) {
  return SRC_LABEL[source] || 'อัปโหลด'
}

/** แยก note จากรายการเวอร์ชัน */
export function noteParts(v) {
  return parseChangeNote(v?.note || '')
}

/** คอลัมน์ “เรื่อง” ในตารางประวัติ */
export function changeTopic(v, np = noteParts(v)) {
  return np.topic || (v?.source === 'original' ? 'ต้นฉบับ' : srcLabel(v?.source))
}

export function fmtDate(iso) {
  try {
    return new Date(iso).toLocaleDateString('th-TH', { day: 'numeric', month: 'long', year: 'numeric' })
  } catch {
    return iso || '—'
  }
}

export function fmtWhen(iso) {
  try {
    return new Date(iso).toLocaleString('th-TH')
  } catch {
    return iso || ''
  }
}

/** สแนปช็อตเนื้อหาตอนกดอัปเดท (ก่อน SignDialog) */
export function captureEditDraft(content, sheet, editorEl) {
  if (content?.kind === 'sheet' && sheet) {
    return {
      draftSheet: { active: sheet.active, rows: sheet.rows.map((r) => r.slice()) },
      draftHtml: '',
    }
  }
  return {
    draftHtml: editorEl ? editorEl.innerHTML : (content?.html || ''),
    draftSheet: null,
  }
}

export function htmlFromDraft(draftHtml, editorEl, content) {
  return draftHtml || (editorEl ? editorEl.innerHTML : content?.html)
}
