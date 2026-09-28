// Production server (Bun) — เสิร์ฟ frontend ที่ build แล้ว + proxy /api และ /converter
// ใช้กับ deploy iso.kgarden.co.th ผ่าน Cloudflare Tunnel
import { resolve, normalize, sep } from 'node:path'
const PORT = Number(process.env.PORT || 8090)
const API = process.env.API_TARGET || 'http://127.0.0.1:8080'
const CONV = process.env.CONV_TARGET || 'http://127.0.0.1:8081'
const DIST = new URL('./frontend/dist/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')
const DIST_ABS = resolve(DIST)

const MIME = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg',
  '.ico': 'image/x-icon', '.woff2': 'font/woff2', '.woff': 'font/woff', '.map': 'application/json',
}
const extOf = (p) => { const i = p.lastIndexOf('.'); return i < 0 ? '' : p.slice(i).toLowerCase() }

Bun.serve({
  port: PORT,
  idleTimeout: 120,
  async fetch(req) {
    const url = new URL(req.url)
    const path = url.pathname

    // proxy API + converter (คง path เดิม)
    if (path.startsWith('/api')) return proxy(req, API)
    if (path.startsWith('/converter')) return proxy(req, CONV, '/converter')

    // static + SPA fallback (จำกัด path ให้อยู่ใน DIST เท่านั้น กัน path traversal)
    let rel
    try { rel = decodeURIComponent(path === '/' ? '/index.html' : path) }
    catch { return new Response('Bad Request', { status: 400 }) }
    if (rel.includes('\0') || rel.includes('\\')) return new Response('Forbidden', { status: 403 })
    const target = resolve(DIST_ABS, '.' + normalize(rel))
    if (target !== DIST_ABS && !target.startsWith(DIST_ABS + sep)) return new Response('Forbidden', { status: 403 })
    let file = Bun.file(target)
    if (!(await file.exists())) file = Bun.file(resolve(DIST_ABS, 'index.html')) // SPA fallback
    const ext = extOf(target)
    return new Response(file, { headers: { 'Content-Type': MIME[ext] || 'application/octet-stream' } })
  },
})

async function proxy(req, target, strip) {
  const u = new URL(req.url)
  let p = u.pathname
  if (strip) p = p.slice(strip.length) || '/'
  const dest = target + p + u.search
  const init = { method: req.method, headers: req.headers, redirect: 'manual' }
  if (!['GET', 'HEAD'].includes(req.method)) init.body = await req.arrayBuffer()
  try {
    return await fetch(dest, init)
  } catch (e) {
    return new Response(JSON.stringify({ error: 'upstream ไม่ตอบสนอง: ' + e.message }), { status: 502, headers: { 'Content-Type': 'application/json' } })
  }
}

console.log(`  ✅ iso production server: http://127.0.0.1:${PORT}  (api->${API}, conv->${CONV})`)
