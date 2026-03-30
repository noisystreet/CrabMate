import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  build: {
    outDir: 'dist',
  },
  server: {
    proxy: {
      // 显式对象：避免开发代理对 POST SSE（/chat/stream）做整块缓冲，导致浏览器侧看不到逐段输出
      '/chat': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: true,
        configure(proxy) {
          proxy.on('proxyRes', (proxyRes, req) => {
            if (req.url?.includes('/stream')) {
              delete proxyRes.headers['content-length']
            }
          })
        },
      },
      '/status': 'http://127.0.0.1:8080',
      '/workspace': 'http://127.0.0.1:8080',
      '/health': 'http://127.0.0.1:8080',
    },
  },
})
