// เรียก Rust API (ผ่าน Vite proxy /api และ /converter)
export const api = {
  docs: () => fetch('/api/docs').then((r) => r.json()),
  stats: () => fetch('/api/stats').then((r) => r.json()),
  overview: () => fetch('/api/overview').then((r) => r.json()),
  versions: (code) => fetch(`/api/docs/${encodeURIComponent(code)}/versions`).then((r) => r.json()),
  content: (code, v) =>
    fetch(`/api/docs/${encodeURIComponent(code)}/content${v ? '?v=' + v : ''}`).then((r) => r.json()),
  save: (code, payload) =>
    fetch(`/api/docs/${encodeURIComponent(code)}/versions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    }).then(async (r) => ({ ok: r.ok, body: await r.json() })),
}

export const fileUrl = (code, v, dl) =>
  `/api/docs/${encodeURIComponent(code)}/file?v=${v}${dl ? '&dl=1' : ''}`
export const ovFileUrl = (p, dl) =>
  `/api/overview/file?path=${encodeURIComponent(p)}${dl ? '&dl=1' : ''}`

export const b64ToBytes = (b64) => {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}
export const bytesToB64 = (bytes) => {
  let s = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) s += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk))
  return btoa(s)
}
