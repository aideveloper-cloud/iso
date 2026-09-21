/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{svelte,js}'],
  theme: {
    extend: {
      colors: {
        // ธีมสไตล์ KO AI FIRST — navy น้ำเงินเข้ม + เขียวสด
        brand: {
          DEFAULT: '#0B3679', // navy หลัก (KO)
          600: '#1552A8', // navy อ่อนลงสำหรับ hover/ไล่เฉด
          700: '#08285C', // navy เข้มสุด
          light: '#EAF1FB', // พื้นหลังฟ้าอ่อน
          pale: '#D9E4F5', // แท็ก/ชิป
        },
        accent: {
          DEFAULT: '#96BF4F', // เขียว KO
          600: '#7CA53B', // เขียวเข้ม (hover)
          light: '#EEF6E0', // พื้นหลังเขียวอ่อน
        },
        ink: '#212529', // ตัวอักษรหลัก (KO)
        mut: '#667085',
        faint: '#9AA3B2',
        surface: '#ffffff',
        line: '#E6EAF0',
        canvas: '#F5F7FA',
        warn: { DEFAULT: '#B7791F', bg: '#FDF3DE' },
        ok: { DEFAULT: '#1F9D63', bg: '#E6F7EE' },
        danger: { DEFAULT: '#C0504D', bg: '#FBEAEA' },
      },
      fontFamily: {
        sans: ['Prompt', 'Segoe UI', 'Tahoma', 'sans-serif'],
      },
      borderRadius: {
        DEFAULT: '10px',
        sm: '6px',
        lg: '14px',
      },
      boxShadow: {
        sm: '0 1px 2px rgba(16,24,40,.06)',
        DEFAULT: '0 1px 4px rgba(16,24,40,.08)',
        lg: '0 12px 34px rgba(16,24,40,.22)',
      },
      keyframes: {
        spin: { to: { transform: 'rotate(360deg)' } },
      },
    },
  },
  plugins: [],
}
