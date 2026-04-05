import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { viteSingleFile } from 'vite-plugin-singlefile'
import { readFileSync, writeFileSync } from 'fs'
import { resolve } from 'path'

function injectCspNonce() {
  return {
    name: 'inject-csp-nonce',
    apply: 'build' as const,
    closeBundle() {
      const outFile = resolve(__dirname, 'dist/index.html')
      let html = readFileSync(outFile, 'utf-8')
      // Add nonce to generated <style> and <script> tags (skip tags that already have one)
      html = html.replace(/<style(?![^>]*nonce)/g, '<style nonce="__CSP_NONCE__"')
      html = html.replace(/<script(?![^>]*nonce)/g, '<script nonce="__CSP_NONCE__"')
      writeFileSync(outFile, html)
    },
  }
}

export default defineConfig({
  plugins: [react(), viteSingleFile(), injectCspNonce()],
  build: {
    outDir: 'dist',
    target: 'es2020',
    assetsInlineLimit: 100_000_000,
    rollupOptions: {
      output: {
        inlineDynamicImports: true,
      },
    },
  },
})
