import { defineConfig } from "vite";

export default defineConfig({
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
