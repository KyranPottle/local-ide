import solid from "vite-plugin-solid"
import { defineConfig } from "vitest/config"

export default defineConfig({
  plugins: [solid()],
  server: {
    // The engine accepts only the origin in its public_origin setting, so the address stays fixed
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": { target: "http://127.0.0.1:7878", ws: true },
    },
  },
  test: {
    // The Playwright specs in e2e/ run with pnpm e2e
    include: ["src/**/*.test.ts"],
    // Without this, vite-plugin-solid selects jsdom, which is not a dependency of this package
    environment: "node",
    // The token contrast test reads tokens.css, which Vitest replaces with an empty module by default
    css: true,
  },
})
