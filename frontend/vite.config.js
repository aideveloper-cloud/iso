import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// dev proxy: หน้าเว็บเรียก same-origin แล้ว vite ส่งต่อไป Rust (API) และ Go (converter)
// host: true → เปิดให้เครื่องอื่นใน LAN เข้าผ่าน IP ของเครื่องนี้
export default defineConfig({
  plugins: [svelte()],
  server: {
    host: true,
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': 'http://127.0.0.1:8080',
      '/converter': {
        target: 'http://127.0.0.1:8081',
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/converter/, ''),
      },
    },
  },
  preview: {
    host: true,
    port: 5173,
    strictPort: true,
  },
})
