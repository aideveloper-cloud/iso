/** ความพร้อมรับการตรวจประเมิน — ป้ายสถานะและการส่งออกรายงาน */

export const RD_STATUS = { pass: 'ผ่าน', warning: 'ต้องติดตาม', critical: 'วิกฤต' }
export const RD_ORDER = { critical: 0, warning: 1, pass: 2 }

export const byStatus = (a, b) => RD_ORDER[a.status] - RD_ORDER[b.status]

/** กันสูตรใน Excel (=, +, -, @ และอักขระควบคุมนำหน้า) แล้วครอบด้วยเครื่องหมายคำพูด */
function cell(v) {
  let s = v == null ? '' : String(v)
  if (/^[\s]*[=+\-@\t\r]/.test(s)) s = "'" + s
  return '"' + s.replace(/"/g, '""') + '"'
}

function download(name, text, type) {
  const url = URL.createObjectURL(new Blob([text], { type }))
  const a = document.createElement('a')
  a.href = url
  a.download = name
  a.click()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}

const stamp = (r) => (r.asOf || new Date().toISOString()).slice(0, 10)

/** CSV (มี BOM ให้ Excel อ่านภาษาไทยถูก) — รายการตรวจทั้งหมด + วิธีคิดคะแนน */
export function exportCsv(r) {
  const rows = [
    ['รหัส', 'ข้อกำหนด', 'หมวด', 'รายการตรวจ', 'สถานะ', 'รายละเอียด', 'หลักฐาน'],
    ...[...r.controls].sort(byStatus).map((c) => [c.id, c.clause, c.category, c.title, RD_STATUS[c.status], c.detail, c.evidence.join(' ')]),
    [],
    ['คะแนนความพร้อม', `${r.score}%`, `ผ่าน ${r.passed}`, `ต้องติดตาม ${r.warnings}`, `วิกฤต ${r.critical}`, `ทั้งหมด ${r.total}`],
    ['ณ เวลา', r.asOf],
    ['วิธีคิดคะแนน', r.methodology],
  ]
  download(`audit-readiness-${stamp(r)}.csv`, '\ufeff' + rows.map((x) => x.map(cell).join(',')).join('\r\n'), 'text/csv;charset=utf-8')
}

export function exportJson(r) {
  download(`audit-readiness-${stamp(r)}.json`, JSON.stringify(r, null, 2), 'application/json')
}
