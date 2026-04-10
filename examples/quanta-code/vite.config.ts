import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";

export default defineConfig({
  plugins: [wasm()],
  build: {
    target: "esnext",
  },
  server: {
    port: 5173,
    proxy: {
      "/ws": {
        target: "http://localhost:4000",
        ws: true,
      },
      "/api": {
        target: "http://localhost:4000",
      },
    },
  },
});
