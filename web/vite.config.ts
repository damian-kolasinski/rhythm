import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
export default defineConfig({
  plugins: [solid()],
  build: {
    target: "es2022",
    sourcemap: false,
    rolldownOptions: {
      output: {
        entryFileNames: "assets/app.js",
        chunkFileNames: "assets/[name].js",
        assetFileNames: (asset) =>
          asset.names.some((name) => name.endsWith(".woff2"))
            ? "assets/dm-sans.woff2"
            : "assets/app.[ext]",
      },
    },
  },
});
