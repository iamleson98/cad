/**
 * Bridge client: typed access to the in-app E2E bridge
 * (`window.__forgecad`, installed by forge-app debug wasm builds).
 *
 * The bridge refreshes every frame:
 * - `frame()` — heartbeat (a stalled counter = hang)
 * - `widgets()` — every interactive widget's id/label/rect/enabled
 * - `state()` — app-state JSON for assertions
 * - `errors()` — captured Rust panics
 * - `action(name, payload)` — queue features that need file bytes
 */
import { expect, type Page } from "@playwright/test";

export interface Widget {
  id: string;
  label: string;
  kind: string;
  rect: [number, number, number, number];
  enabled: boolean;
}

export interface AppState {
  frame: number;
  doc: string;
  modified: boolean;
  features: number;
  bodies: number | null;
  selected: number;
  status: string;
  evalPending: boolean;
  evalDone: number;
  measureMode: boolean;
  measurePicks: number;
  measureLabel?: string;
  gizmoMode: string;
  paletteOpen: boolean;
  grid: boolean;
  edges: boolean;
  pickMode: string;
  camera: { distance: number; yaw: number; pitch: number; orthographic: boolean };
  errors: number;
}

export class ForgeApp {
  readonly page: Page;
  private consoleErrors: string[] = [];

  constructor(page: Page) {
    this.page = page;
  }

  /** Load the app and wait for the first rendered frame. */
  async boot(): Promise<void> {
    // Collect everything that looks like an app failure.
    this.page.on("pageerror", (err) => this.consoleErrors.push(`pageerror: ${err}`));
    this.page.on("console", (msg) => {
      if (process.env.E2E_VERBOSE) {
        // eslint-disable-next-line no-console
        console.log(`[live:${msg.type()}]`, msg.text().slice(0, 600));
      }
      if (msg.type() === "error") this.consoleErrors.push(`console.error: ${msg.text()}`);
    });

    await this.page.goto("/");
    const canvas = this.page.locator("#forgecad_canvas");
    await expect(canvas).toBeVisible({ timeout: 20_000 });
    // Bridge exists (debug bundle) and the first frame ran.
    await this.page.waitForFunction(
      () => (window as any).__forgecad && (window as any).__forgecad.frame() > 0,
      undefined,
      { timeout: 20_000 },
    );
    // Give the app a moment to settle the initial evaluation.
    await this.waitForFrames(5);
  }

  // ---- Bridge access ----------------------------------------------------

  async frame(): Promise<number> {
    return this.page.evaluate(() => (window as any).__forgecad.frame());
  }

  async widgets(): Promise<Widget[]> {
    const json = await this.page.evaluate(() => (window as any).__forgecad.widgets());
    return JSON.parse(json) as Widget[];
  }

  async state(): Promise<AppState> {
    const json = await this.page.evaluate(() => (window as any).__forgecad.state());
    return JSON.parse(json) as AppState;
  }

  async bridgeErrors(): Promise<string[]> {
    const json = await this.page.evaluate(() => (window as any).__forgecad.errors());
    return JSON.parse(json) as string[];
  }

  async clearErrors(): Promise<void> {
    await this.page.evaluate(() => (window as any).__forgecad.clearErrors());
    this.consoleErrors = [];
  }

  async action(name: string, payload: string): Promise<void> {
    await this.page.evaluate(
      ([n, p]) => (window as any).__forgecad.action(n, p),
      [name, payload] as const,
    );
  }

  // ---- Liveness ----------------------------------------------------------

  /**
   * Wait until at least `n` further frames rendered. egui repaints
   * ON DEMAND — an idle app legitimately stops rendering — so wiggle
   * the pointer (a real user does this constantly) to request frames.
   */
  async waitForFrames(n: number, timeout = 5_000): Promise<void> {
    const start = await this.frame();
    const deadline = Date.now() + timeout;
    let flip = false;
    while (Date.now() < deadline) {
      // Pointer movement is an input event → egui schedules a repaint.
      flip = !flip;
      await this.page.mouse.move(700 + (flip ? 0 : 2), 450);
      try {
        await this.page.waitForFunction(
          (target) => (window as any).__forgecad.frame() >= target,
          start + n,
          { timeout: 500 },
        );
        return;
      } catch {
        // keep wiggling until the deadline
      }
    }
    throw new Error(`heartbeat stalled: no ${n} frames within ${timeout} ms`);
  }

  /**
   * Wait until the evaluation settles. Tolerates FROZEN frames: on
   * wasm the evaluation runs synchronously on the UI thread — the page
   * legitimately blocks for seconds mid-eval — so poll the state
   * directly (evaluate waits through the freeze) instead of requiring
   * heartbeats.
   */
  async waitForEval(timeout = 10_000): Promise<AppState> {
    const t0 = Date.now();
    const start = (await this.state()).evalDone;
    for (;;) {
      const s = await this.state();
      if (!s.evalPending && (s.evalDone > start || Date.now() - t0 > 1_500)) {
        // Settled: let the post-eval frame land (scene/inspector refresh).
        try {
          await this.waitForFrames(2, 3_000);
        } catch {
          // frozen page would have thrown — but state says settled
        }
        return await this.state();
      }
      if (Date.now() - t0 > timeout) {
        throw new Error(
          `evaluation did not settle within ${timeout} ms (state: ${JSON.stringify(s)})`,
        );
      }
      await this.page.waitForTimeout(80);
    }
  }

  // ---- Widget lookup + real clicks ---------------------------------------

  async widget(id: string): Promise<Widget> {
    const w = (await this.widgets()).find((x) => x.id === id);
    if (!w) throw new Error(`widget ${id} not found`);
    return w;
  }

  async widgetPrefix(prefix: string): Promise<Widget> {
    const w = (await this.widgets()).find((x) => x.id.startsWith(prefix));
    if (!w) throw new Error(`no widget with id prefix ${prefix}`);
    return w;
  }

  /** Click a widget at its real rect center (egui points == CSS pixels). */
  async clickWidget(w: Widget): Promise<void> {
    if (!w.enabled) throw new Error(`widget ${w.id} is disabled`);
    const cx = (w.rect[0] + w.rect[2]) / 2;
    const cy = (w.rect[1] + w.rect[3]) / 2;
    await this.page.mouse.click(cx, cy);
    await this.waitForFrames(2);
  }

  async click(id: string): Promise<void> {
    await this.clickWidget(await this.widget(id));
  }

  async clickPrefix(prefix: string): Promise<void> {
    await this.clickWidget(await this.widgetPrefix(prefix));
  }

  async clickLabel(needle: string): Promise<void> {
    const w = (await this.widgets()).find((x) => x.label.includes(needle));
    if (!w) throw new Error(`no widget with label containing ${needle}`);
    await this.clickWidget(w);
  }

  /** Click the 3D viewport at a relative position (0..1 of its rect). */
  async clickViewport(relX: number, relY: number): Promise<void> {
    const vp = await this.widget("viewport");
    const x = vp.rect[0] + (vp.rect[2] - vp.rect[0]) * relX;
    const y = vp.rect[1] + (vp.rect[3] - vp.rect[1]) * relY;
    await this.page.mouse.click(x, y);
    await this.waitForFrames(2);
  }

  async drag(from: { x: number; y: number }, to: { x: number; y: number }): Promise<void> {
    await this.page.mouse.move(from.x, from.y);
    await this.page.mouse.down();
    const steps = 8;
    for (let i = 1; i <= steps; i++) {
      await this.page.mouse.move(
        from.x + ((to.x - from.x) * i) / steps,
        from.y + ((to.y - from.y) * i) / steps,
      );
      await this.waitForFrames(1);
    }
    await this.page.mouse.up();
    await this.waitForFrames(2);
  }

  async viewportDrag(relFrom: [number, number], relTo: [number, number], alt = true): Promise<void> {
    const vp = await this.widget("viewport");
    const [x0, y0] = [vp.rect[0], vp.rect[1]];
    const [w, h] = [vp.rect[2] - vp.rect[0], vp.rect[3] - vp.rect[1]];
    if (alt) await this.page.keyboard.down("Alt");
    await this.drag(
      { x: x0 + w * relFrom[0], y: y0 + h * relFrom[1] },
      { x: x0 + w * relTo[0], y: y0 + h * relTo[1] },
    );
    if (alt) await this.page.keyboard.up("Alt");
  }

  // ---- Health checks ------------------------------------------------------

  /** App is alive and no panics/errors were captured. */
  async expectHealthy(): Promise<void> {
    await this.waitForFrames(2);
    const bridge = await this.bridgeErrors();
    // Distinguish benign from fatal console noise later if needed.
    expect(bridge, "bridge captured no panics").toEqual([]);
    expect(this.consoleErrors, "no console errors").toEqual([]);
  }

  /** Screenshot for visual inspection (test-artifacts/). */
  async snap(name: string): Promise<void> {
    await this.page.screenshot({ path: `test-results/artifacts/${name}.png` });
  }

  // ---- Compound flows ------------------------------------------------------

  /** Open the Solid menu and add a primitive (real menu clicks). */
  async addSolid(kind: string): Promise<void> {
    await this.click("menu:Solid");
    await this.waitForFrames(2);
    await this.click(`solid:${kind}`);
    await this.waitForFrames(3);
  }
}
