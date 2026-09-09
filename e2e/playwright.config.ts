import { defineConfig } from "@playwright/test";

// ForgeCAD browser E2E: headless Chromium against the trunk DEBUG wasm
// bundle (the E2E bridge is compiled into debug builds only — release
// bundles stay clean).
//
// Local fast loop:
//   1. `cd crates/forge-app && trunk build`        (debug, ~40 s warm)
//   2. `cd e2e && npx playwright test`             (starts serve.mjs)
// Or keep `trunk serve` on :8080 for live rebuilds — tests reuse it.
const baseURL = process.env.E2E_BASE_URL ?? "http://127.0.0.1:8080";

export default defineConfig({
  testDir: "./tests",
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: false, // one app instance per page; keep runs deterministic
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: [
    ["list"],
    ["json", { outputFile: "test-results/report.json" }],
    ["html", { outputFolder: "test-results/html", open: "never" }],
  ],
  use: {
    baseURL,
    viewport: { width: 1400, height: 900 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    // Software WebGL (SwiftShader) for headless wgpu; WebGPU/Dawn behind
    // the unsafe flags in case it is available (wgpu prefers it).
    launchOptions: {
      args: [
        "--enable-unsafe-swiftshader",
        "--enable-unsafe-webgpu",
        "--disable-gpu-sandbox",
        // Never throttle a background/headless page: rAF must keep
        // firing so the on-demand egui repaint loop stays responsive.
        "--disable-background-timer-throttling",
        "--disable-backgrounding-occluded-windows",
        "--disable-renderer-backgrounding",
      ],
    },
  },
  outputDir: "test-results/artifacts",
  webServer: {
    command: "node serve.mjs ../crates/forge-app/dist 8080",
    url: baseURL,
    reuseExistingServer: !process.env.CI, // local `trunk serve` wins
    timeout: 60_000,
    stdout: "ignore",
  },
});
