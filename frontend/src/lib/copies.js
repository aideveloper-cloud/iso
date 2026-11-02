// สำเนาเอกสาร (สำเนาควบคุมที่พิมพ์ / ไฟล์ต้นฉบับที่ดาวน์โหลด) — ป้ายกำกับใช้ร่วมกันระหว่างหน้า DCC กับหน้าเอกสาร
export const COPY_KIND = { print: 'สำเนาควบคุม (พิมพ์)', file: 'ไฟล์ที่ดาวน์โหลด' }

export const COPY_STATUS = {
  active: 'ใช้งาน',
  recall: 'ต้องเรียกคืน',
  returned: 'คืนแล้ว',
  destroyed: 'ทำลายแล้ว',
  lost: 'สูญหาย',
}

/** คลาสสีของป้ายสถานะ (ใช้ .ia-res) */
export function copyTone(c) {
  if (c.status === 'recall') return c.overdue ? 'r-major' : 'r-minor'
  if (c.status === 'active') return 'r-conform'
  if (c.status === 'lost') return 'r-major'
  return ''
}

/** ผลการปิดสำเนา — ไฟล์ที่ดาวน์โหลดไม่มี "คืน" (ลบไฟล์ = ทำลาย) */
export function closeChoices(kind) {
  return kind === 'file'
    ? [['destroyed', 'ลบไฟล์ในเครื่องแล้ว'], ['lost', 'ไม่พบไฟล์/ส่งต่อให้ผู้อื่นแล้ว']]
    : [['returned', 'คืน DCC แล้ว'], ['destroyed', 'ทำลายแล้ว'], ['lost', 'สูญหาย']]
}
