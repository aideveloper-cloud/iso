/** ทบทวนฝ่ายบริหาร (ISO 9001:2015 ข้อ 9.3) — ป้ายสถานะและสิทธิ์ */

export const MR_STATUS = {
  draft: 'ร่าง',
  review: 'รอตรวจสอบ',
  approve: 'รออนุมัติ',
  monitoring: 'ติดตามงาน',
  closed: 'ปิดแล้ว',
  superseded: 'ถูกแทนที่',
}
export const MR_STEPS = ['draft', 'review', 'approve', 'monitoring', 'closed']
export const HEALTH = { good: 'เป็นไปตามแผน', attention: 'ต้องติดตาม', critical: 'วิกฤต', unreviewed: 'ข้อมูลไม่พอ' }
export const ACT_STATUS = { not_started: 'ยังไม่เริ่ม', in_progress: 'กำลังดำเนินการ', completed: 'เสร็จแล้ว', cancelled: 'ยกเลิก' }
export const EFFECT = { pending: 'รอทวนสอบ', effective: 'มีประสิทธิผล', not_effective: 'ไม่มีประสิทธิผล' }
export const APPROVAL = { not_required: 'ไม่ต้องอนุมัติงบ', pending: 'รออนุมัติงบ', approved: 'อนุมัติงบแล้ว' }
export const PRIORITY = { high: 'สูง', medium: 'ปานกลาง', low: 'ต่ำ' }
export const SIG_STAGE = { prepared: 'ผู้จัดทำ (เลขานุการ)', reviewed: 'ผู้ตรวจสอบ', approved: 'ผู้อนุมัติ (ฝ่ายบริหาร)', closed: 'ปิดการประชุม', revision: 'ฉบับแก้ไข' }
export const SIG_ACTION = { submit: 'ส่งตรวจสอบ', pass: 'ตรวจสอบผ่าน', return: 'ตีกลับ', approve: 'อนุมัติ', close: 'ปิดการประชุม', revise: 'สร้างฉบับแก้ไข' }

export const today = () => new Date(Date.now() + 7 * 3600e3).toISOString().slice(0, 10)
export const isLate = (a) => ['not_started', 'in_progress'].includes(a.status) && a.dueDate && a.dueDate < today()

/** ตารางสิทธิ์ตามบทบาท (ตรงกับการตรวจฝั่ง server) */
export const ROLES = {
  viewer: 'ผู้อ่าน (Viewer)',
  preparer: 'ผู้จัดทำ (Preparer)',
  reviewer: 'ผู้ตรวจสอบ (Reviewer)',
  approver: 'ผู้อนุมัติ (Approver)',
  admin: 'ผู้ดูแลระบบ (Admin)',
}
export const PERMISSIONS = [
  ['ดูเอกสาร รายงาน และผลความพร้อม Audit', ['viewer', 'preparer', 'reviewer', 'approver', 'admin']],
  ['ส่งคำขอแก้ไขเอกสาร (DCR) · กรอกแบบฟอร์ม · เปิด CAR/DAR/PAR', ['preparer', 'reviewer', 'approver', 'admin']],
  ['ทบทวนฝ่ายบริหาร: สร้างการประชุม บันทึกข้อมูลนำเข้า มติ งานติดตาม และส่งตรวจสอบ', ['preparer', 'reviewer', 'approver', 'admin']],
  ['ทบทวนฝ่ายบริหาร: อัปเดตความคืบหน้า/หลักฐานงานที่ตนรับผิดชอบ', ['preparer', 'reviewer', 'approver', 'admin']],
  ['ตรวจสอบ DCR · แบบฟอร์ม · การประชุมทบทวน (ไม่ใช่ของตนเอง)', ['reviewer', 'approver', 'admin']],
  ['ทวนสอบประสิทธิผลงานติดตาม (ไม่ใช่งานของตนเอง) · ยกเลิกงาน', ['reviewer', 'approver', 'admin']],
  ['บันทึกผลความพร้อม Audit', ['reviewer', 'approver', 'admin']],
  ['อนุมัติ DCR · แบบฟอร์ม · การประชุมทบทวน (ไม่ใช่ที่ตนจัดทำ/ตรวจสอบ) · อนุมัติงบ', ['approver', 'admin']],
  ['ปิดการประชุมทบทวน · ประกาศยกเลิกใช้เอกสาร', ['approver', 'admin']],
  ['วางแผนการตรวจติดตามภายใน · ดูบันทึกการใช้งาน', ['reviewer', 'approver', 'admin']],
  ['ตรวจติดตามภายใน: บันทึกผล/เปิด CAR (เฉพาะทีมผู้ตรวจที่ขึ้นทะเบียน ไม่ตรวจแผนกตนเอง)', ['preparer', 'reviewer', 'approver', 'admin']],
  ['ปิดการตรวจติดตามภายใน (ไม่อยู่ในทีมผู้ตรวจ)', ['approver', 'admin']],
  ['จัดการผู้ใช้และสิทธิ์ · นำเอกสารที่ยกเลิกกลับมาใช้', ['admin']],
]
