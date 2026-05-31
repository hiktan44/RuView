import { WasmInferenceEngine, isWasmSupported } from '@/inference/wasmInferenceEngine';
import { createInferenceEngine } from '@/inference';
import type { FeatureSet, SensingFrame } from '@/types/sensing';

const makeFrame = (features: Partial<FeatureSet>): SensingFrame => ({
  type: 'sensing_update',
  timestamp: 1000,
  nodes: [],
  features: {
    mean_rssi: -45,
    variance: 0.1,
    motion_band_power: 0.1,
    breathing_band_power: 0.1,
    spectral_entropy: 0,
    ...features,
  },
  classification: { motion_level: 'absent', presence: false, confidence: 0 },
  signal_field: { grid_size: [20, 1, 20], values: [] },
});

describe('WasmInferenceEngine', () => {
  it('reports WASM support based on the runtime', () => {
    // Jest/node provides WebAssembly, so support should be true here.
    expect(isWasmSupported()).toBe(typeof WebAssembly === 'object');
  });

  it('init returns false when WebAssembly is unavailable (simulated Hermes)', async () => {
    const original = (globalThis as Record<string, unknown>).WebAssembly;
    // Simulate a runtime without WebAssembly (e.g. native Hermes).
    delete (globalThis as Record<string, unknown>).WebAssembly;
    try {
      const engine = new WasmInferenceEngine();
      await expect(engine.init()).resolves.toBe(false);
    } finally {
      (globalThis as Record<string, unknown>).WebAssembly = original;
    }
  });

  it('init returns true with no loader (WASM supported) and delegates to JS', async () => {
    const engine = new WasmInferenceEngine();
    await expect(engine.init()).resolves.toBe(true);
    expect(engine.isModelLive).toBe(false);
    const result = engine.infer(makeFrame({ variance: 0.1 }));
    // Behaviour is correct (JS heuristics) but tagged as the wasm engine.
    expect(result.backend).toBe('wasm');
    expect(result.presence.present).toBe(true);
  });

  it('stays usable when the model loader throws', async () => {
    const engine = new WasmInferenceEngine(async () => {
      throw new Error('bad wasm');
    });
    await expect(engine.init()).resolves.toBe(true);
    expect(engine.isModelLive).toBe(false);
    expect(() => engine.infer(makeFrame({ variance: 0.1 }))).not.toThrow();
  });

  it('marks the model live when the loader returns an infer export', async () => {
    const engine = new WasmInferenceEngine(async () => ({ infer: () => 0 }));
    await engine.init();
    expect(engine.isModelLive).toBe(true);
    // Even with a live (stub) model, output is never fabricated — falls back.
    const result = engine.infer(makeFrame({ variance: 0.1 }));
    expect(result.backend).toBe('wasm');
  });

  it('dispose resets model state', async () => {
    const engine = new WasmInferenceEngine(async () => ({ infer: () => 0 }));
    await engine.init();
    engine.dispose();
    expect(engine.isModelLive).toBe(false);
  });
});

describe('createInferenceEngine factory', () => {
  it('returns a JS engine for preference "js"', () => {
    expect(createInferenceEngine({ preference: 'js' }).kind).toBe('js');
  });

  it('returns a WASM engine for preference "wasm"', () => {
    expect(createInferenceEngine({ preference: 'wasm' }).kind).toBe('wasm');
  });

  it('auto picks WASM when supported', () => {
    const engine = createInferenceEngine({ preference: 'auto' });
    expect(engine.kind).toBe(isWasmSupported() ? 'wasm' : 'js');
  });
});
