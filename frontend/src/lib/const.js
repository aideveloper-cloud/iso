export const DEPTS = ['MR', 'DCC', 'SA', 'PU', 'HR', 'PD', 'QC', 'MN', 'WH']
export const DEPTNAME = {
  MR: 'QMR ฝ่ายบริหารคุณภาพ', DCC: 'DCC ควบคุมเอกสาร', SA: 'SALES ขายและการตลาด',
  PU: 'PU จัดซื้อ', HR: 'HR ทรัพยากรมนุษย์', PD: 'PD ผลิต', QC: 'QC ควบคุมคุณภาพ',
  MN: 'MN ซ่อมบำรุง', WH: 'WH คลังสินค้า',
}
export const PROC_READY = ['MR', 'QC']
export const TYPE_ORDER = [
  ['QM', 'คู่มือคุณภาพ (QM)'], ['QP', 'ระเบียบปฏิบัติ (QP)'],
  ['WI', 'วิธีปฏิบัติงาน (WI)'], ['FM', 'แบบฟอร์ม (FM)'],
]
export const fmtKB = (b) =>
  !b ? '' : b >= 1048576 ? (b / 1048576).toFixed(1) + ' MB' : Math.max(1, Math.round(b / 1024)) + ' KB'

export function fileExt(f) {
  return (f.ext || f.path?.split('.').pop() || '').toLowerCase()
}

/** Word/Excel ที่แก้บนหน้าเว็บได้ (ตรงกับ API editable) */
export function isWebEditable(e) {
  return ['docx', 'xls', 'xlsx'].includes((e || '').toLowerCase())
}

export function fileKind(e) {
  if (['xls', 'xlsx'].includes(e)) return { t: 'Excel', c: 'FM', icon: 'file-spreadsheet', label: 'ตาราง Excel' }
  if (['doc', 'docx'].includes(e)) return { t: 'Word', c: 'QP', icon: 'book-open', label: 'เอกสาร Word' }
  if (e === 'pdf') return { t: 'PDF', c: 'WI', icon: 'file-text', label: 'ไฟล์ PDF' }
  if (['jpg', 'jpeg', 'png'].includes(e)) return { t: 'รูป', c: 'QM', icon: 'image', label: 'รูปภาพ' }
  return { t: e.toUpperCase(), c: '', icon: 'file-text', label: e.toUpperCase() }
}

export function matchesQuery(d, q) {
  if (!q) return true
  const n = q.toLowerCase()
  return d.code.toLowerCase().includes(n) || d.name.toLowerCase().includes(n)
}

export function sortByField(rows, key, asc) {
  return rows.slice().sort((a, b) => {
    const x = (a[key] || '') + '', y = (b[key] || '') + ''
    if (x === y) return 0
    const dir = x < y ? -1 : 1
    return asc ? dir : -dir
  })
}
