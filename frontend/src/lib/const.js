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
