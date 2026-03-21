import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "0.0.0.0",
    port: 4173,
    proxy: {
      "/xrpc": {
        target: "http://127.0.0.1:3002",
        changeOrigin: true,
      },
    },
  },
});
