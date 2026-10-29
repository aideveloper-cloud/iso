import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// dev proxy: หน้าเว็บเรียก same-origin แล้ว vite ส่งต่อไป Rust (API)
// host: true → เปิดให้เครื่องอื่นใน LAN เข้าผ่าน IP ของเครื่องนี้
export default defineConfig({
  plugins: [svelte()],
  server: {
    host: true,
    port: 5173,
    strictPort: true,
    proxy: {
      // ISO_API: ชี้ไป API ทดสอบได้ (ค่าเริ่มต้น API บนเครื่องนี้)
      '/api': process.env.ISO_API || 'http://127.0.0.1:8085',
    },
  },
  preview: {
    host: true,
    port: 5173,
    strictPort: true,
  },
})
