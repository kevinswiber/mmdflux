import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";

const browserTextMetricsRoot = fileURLToPath(
  new URL("../packages/mmds-browser-text-metrics/src/", import.meta.url),
);
const localMmdsWasm = fileURLToPath(
  new URL("./src/wasm-pkg/mmdflux_wasm.js", import.meta.url),
);

export default defineConfig({
  plugins: [wasm()],
  resolve: {
    alias: [
      {
        find: /^@mmds\/browser-text-metrics$/,
        replacement: `${browserTextMetricsRoot}index.ts`,
      },
      {
        find: /^@mmds\/browser-text-metrics\/(.+)$/,
        replacement: `${browserTextMetricsRoot}$1.ts`,
      },
      {
        find: /^@mmds\/wasm$/,
        replacement: localMmdsWasm,
      },
    ],
  },
  worker: {
    format: "es",
    plugins: () => [wasm()],
  },
  test: {
    environment: "jsdom",
  },
});
