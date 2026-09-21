import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// dev proxy: หน้าเว็บเรียก same-origin แล้ว vite ส่งต่อไป Rust (API) และ Go (converter)
export default defineConfig({
  plugins: [svelte()],
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://localhost:8080',
      '/converter': {
        target: 'http://localhost:8081',
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/converter/, ''),
      },
    },
  },
})
