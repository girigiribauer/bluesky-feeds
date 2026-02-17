import { defineConfig } from 'vite'
import solid from 'vite-plugin-solid'

export default defineConfig({
  plugins: [solid()],
  server: {
    port: 5173,
    proxy: {
      '/oauth': 'http://localhost:3000',
      '/privatelist': 'http://localhost:3000',
      '/client-metadata.json': 'http://localhost:3000',
    },
  },
})
