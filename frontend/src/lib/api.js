// เรียก Rust API (ผ่าน Vite proxy /api และ /converter)
function authHeaders(extra = {}) {
  const headers = { ...extra }
  const t = localStorage.getItem('isoToken') || ''
  if (t) headers.Authorization = 'Bearer ' + t
  return headers
}

function noteUnauth() {
  localStorage.removeItem('isoToken')
  window.dispatchEvent(new Event('iso-unauth'))
}

async function send(url, opts = {}) {
  const headers = authHeaders(opts.headers || {})
  const r = await fetch(url, { ...opts, headers })
  if (r.status === 401) {
    noteUnauth()
    const body = await r.json().catch(() => ({ error: 'กรุณาเข้าสู่ระบบ' }))
    const err = new Error(body.error || 'กรุณาเข้าสู่ระบบ')
    err.status = 401
    throw err
  }
  return r
}

async function postJson(url, payload, method = 'POST') {
  const r = await fetch(url, {
    method,
    headers: authHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify(payload ?? {}),
  })
  if (r.status === 401) noteUnauth()
  const body = await r.json().catch(() => ({}))
  return { ok: r.ok, status: r.status, body }
}

export const api = {
  docs: () => send('/api/docs').then((r) => r.json()),
  stats: () => send('/api/stats').then((r) => r.json()),
  overview: () => send('/api/overview').then((r) => r.json()),
  overviewContent: (path, v) =>
    send(`/api/overview/content?path=${encodeURIComponent(path)}${v ? '&v=' + v : ''}`).then((r) => r.json()),
  overviewHistory: (path) => send(`/api/overview/history?path=${encodeURIComponent(path)}`).then((r) => r.json()),
  overviewLatest: () => send('/api/overview/latest').then((r) => r.json()),
  overviewSave: (payload) => postJson('/api/overview/save', payload),
  versions: (code) => send(`/api/docs/${encodeURIComponent(code)}/versions`).then((r) => r.json()),
  content: (code, v) =>
    send(`/api/docs/${encodeURIComponent(code)}/content${v ? '?v=' + v : ''}`).then((r) => r.json()),
  // คำขอเปลี่ยนแปลงเอกสาร (DCR) — เอกสารในทะเบียนแก้ได้ผ่านขั้นตรวจสอบ/อนุมัติเท่านั้น
  docChanges: (code) => send(`/api/docs/${encodeURIComponent(code)}/changes`).then((r) => r.json()),
  changeCreate: (code, payload) => postJson(`/api/docs/${encodeURIComponent(code)}/changes`, payload),
  changes: () => send('/api/changes').then((r) => r.json()),
  change: (id) => send(`/api/changes/${id}`).then((r) => r.json()),
  changeContent: (id) => send(`/api/changes/${id}/content`).then((r) => r.json()),
  changeAct: (id, action, comment, pin) => postJson(`/api/changes/${id}/act`, { action, comment, pin }),
  changeResubmit: (id, payload) => postJson(`/api/changes/${id}/resubmit`, payload),
  changeCancel: (id, reason) => postJson(`/api/changes/${id}/cancel`, { reason }),
  docObsolete: (code, reason, pin) => postJson(`/api/docs/${encodeURIComponent(code)}/obsolete`, { reason, pin }),
  docReinstate: (code, reason, pin) => postJson(`/api/docs/${encodeURIComponent(code)}/reinstate`, { reason, pin }),
  actions: () => send('/api/actions').then((r) => r.json()),
  readiness: () => send('/api/readiness').then((r) => r.json()),
  readinessRuns: () => send('/api/readiness/runs').then((r) => r.json()),
  readinessSave: () => postJson('/api/readiness/runs', {}),
  // ทบทวนฝ่ายบริหาร (9.3)
  mrList: () => send('/api/mr').then((r) => r.json()),
  mrCreate: (fields) => postJson('/api/mr', fields),
  mrGet: (id) => send(`/api/mr/${id}`).then((r) => r.json()),
  mrUpdate: (id, version, fields) => postJson(`/api/mr/${id}`, { version, fields }, 'PUT'),
  mrInput: (id, version, input) => postJson(`/api/mr/${id}/input`, { version, input }, 'PUT'),
  mrSync: (id, version) => postJson(`/api/mr/${id}/sync`, { version }),
  mrAct: (id, version, action, comment, pin) => postJson(`/api/mr/${id}/act`, { version, action, comment, pin }),
  mrRevise: (id, version, reason) => postJson(`/api/mr/${id}/revision`, { version, reason }),
  mrActionCreate: (id, version, fields) => postJson(`/api/mr/${id}/actions`, { version, fields }),
  mrActionUpdate: (aid, version, fields) => postJson(`/api/mr/actions/${aid}`, { version, fields }, 'PUT'),
  mrExport: (id) => send(`/api/mr/${id}/export`).then((r) => r.json()),
  actionCreate: (payload) => postJson('/api/actions', payload),
  action: (id) => send(`/api/actions/${id}`).then((r) => r.json()),
  actionUpdate: (id, version, fields) => postJson(`/api/actions/${id}`, { version, fields }, 'PUT'),
  actionAct: (id, version, action, comment, pin, effective) => postJson(`/api/actions/${id}/act`, { version, action, comment, pin, effective }),
  // Training matrix (7.2)
  trainingMatrix: (dept) => send('/api/training/matrix' + (dept ? '?dept=' + dept : '')).then((r) => r.json()),
  trainingHistory: (code) => send(`/api/training/history/${encodeURIComponent(code)}`).then((r) => r.json()),
  trainingReqCreate: (f) => postJson('/api/training/reqs', f),
  trainingReqUpdate: (id, f) => postJson(`/api/training/reqs/${id}`, f, 'PUT'),
  trainingRecord: (payload) => postJson('/api/training/records', payload),
  // การแจกจ่ายและรับทราบเอกสาร (7.5.3)
  acksMine: () => send('/api/acks/mine').then((r) => r.json()),
  acksOverview: (status) => send('/api/acks' + (status ? '?status=' + status : '')).then((r) => r.json()),
  acksBulk: (codes) => postJson('/api/acks/bulk', { codes }),
  ackSign: (id, pin, comment) => postJson(`/api/acks/${id}/ack`, { pin, comment }),
  docAcks: (code) => send(`/api/docs/${encodeURIComponent(code)}/acks`).then((r) => r.json()),
  docDistribution: (code, targets) => postJson(`/api/docs/${encodeURIComponent(code)}/distribution`, { targets }, 'PUT'),
  docAckRequest: (code) => postJson(`/api/docs/${encodeURIComponent(code)}/acks/request`, {}),
  // ตรวจติดตามภายใน (9.2)
  iaList: (year) => send('/api/ia' + (year ? '?year=' + year : '')).then((r) => r.json()),
  iaCreate: (fields) => postJson('/api/ia', fields),
  iaGet: (id) => send(`/api/ia/${id}`).then((r) => r.json()),
  iaUpdate: (id, version, fields) => postJson(`/api/ia/${id}`, { version, fields }, 'PUT'),
  iaAct: (id, version, action, comment, pin) => postJson(`/api/ia/${id}/act`, { version, action, comment, pin }),
  iaItem: (id, item) => postJson(`/api/ia/${id}/items`, item),
  iaRaiseCar: (id, iid, payload) => postJson(`/api/ia/${id}/items/${iid}/car`, payload || {}),
  iaSetAuditor: (code, qualified) => postJson('/api/ia/auditors', { code, qualified }),
  auditLog: (q) => send('/api/audit-log?' + new URLSearchParams(q || {})).then((r) => r.json()),
  login: (empCode, pin) => postJson('/api/auth/login', { empCode, pin }),
  logout: () => postJson('/api/auth/logout', {}),
  me: () => send('/api/auth/me').then((r) => r.json()),
  changePin: (oldPin, newPin) => postJson('/api/auth/pin', { old: oldPin, new: newPin }),
  employees: (q, dept, all) => {
    const p = new URLSearchParams()
    if (q) p.set('q', q)
    if (dept) p.set('dept', dept)
    if (all) p.set('all', '1')
    return send('/api/employees?' + p.toString()).then((r) => r.json())
  },
  employeeSave: (payload) => postJson('/api/employees', payload),
  schemas: () => send('/api/forms/schemas').then((r) => r.json()),
  schema: (code) => send(`/api/forms/schemas/${encodeURIComponent(code)}`).then((r) => r.json()),
  schemaSave: (payload) => postJson('/api/forms/schemas', payload),
  formRecords: (q) => send('/api/forms/records?' + new URLSearchParams(q || {})).then((r) => r.json()),
  formCreate: (formCode) => postJson('/api/forms/records', { formCode }),
  formGet: (id) => send(`/api/forms/records/${id}?origin=${encodeURIComponent(location.origin)}`).then((r) => r.json()),
  formSave: (id, data) => postJson(`/api/forms/records/${id}`, { data }, 'PUT'),
  formSubmit: (id) => postJson(`/api/forms/records/${id}/submit`, {}),
  formAct: (id, action, comment) => postJson(`/api/forms/records/${id}/act`, { action, comment }),
  formCancel: (id, reason) => postJson(`/api/forms/records/${id}/cancel`, { reason }),
  formAmend: (id, reason) => postJson(`/api/forms/records/${id}/amend`, { reason }),
  formPending: () => send('/api/forms/pending').then((r) => r.json()),
  formStats: () => send('/api/forms/stats').then((r) => r.json()),
  formTraining: (q) => send('/api/forms/training?' + new URLSearchParams({ q: q || '' })).then((r) => r.json()),
  formPlaces: (form) => send('/api/forms/places?form=' + encodeURIComponent(form || '')).then((r) => r.json()),
  formPrevious: (form) => send('/api/forms/previous?form=' + encodeURIComponent(form || '')).then((r) => r.json()),
  formFile: (id, filename, data) => postJson(`/api/forms/records/${id}/files`, { filename, data }),
  formPrint: async (id) => {
    const r = await send(`/api/forms/records/${id}/print?origin=${encodeURIComponent(location.origin)}`)
    return r.text()
  },
  checkinInfo: (token) => fetch('/api/public/checkin/' + encodeURIComponent(token)).then((r) => r.json()),
  checkin: (payload) => postJson('/api/public/checkin', payload),
}

// ลิงก์ดาวน์โหลด/พรีวิวใช้คุกกี้ HttpOnly ที่ server ออกให้ตอน login (ไม่ใส่ token ใน URL — กันรั่วใน log/ประวัติเบราว์เซอร์)
export const fileUrl = (code, v, dl) =>
  (`/api/docs/${encodeURIComponent(code)}/file?v=${v}${dl ? '&dl=1' : ''}`)
export const pdfUrl = (code, v) => (`/api/docs/${encodeURIComponent(code)}/pdf?v=${v}`)
export const ovPdfUrl = (p, v) => (`/api/overview/pdf?path=${encodeURIComponent(p)}${v ? `&v=${v}` : ''}`)
export const changeFileUrl = (id, dl) => (`/api/changes/${id}/file${dl ? '?dl=1' : ''}`)
export const ovFileUrl = (p, dl) =>
  (`/api/overview/file?path=${encodeURIComponent(p)}${dl ? '&dl=1' : ''}`)

export const b64ToBytes = (b64) => {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}
export const bytesToB64 = (bytes) => {
  let s = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    s += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk))
  }
  return btoa(s)
}
