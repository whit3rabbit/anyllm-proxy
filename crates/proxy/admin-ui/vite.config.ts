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

      // No /g flag — first match only. React's bundle contains a literal
      // `innerHTML="<script><\/script>"` string; the global regex matched it and
      // inserted nonce="..." with double-quotes inside the JS string, producing a
      // syntax error that silently prevented the module from loading (blank page).
      html = html
        .replace(/<script(?![^>]*nonce)/, '<script nonce="__CSP_NONCE__"')
        .replace(/<style(?![^>]*nonce)/, '<style nonce="__CSP_NONCE__"')

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
