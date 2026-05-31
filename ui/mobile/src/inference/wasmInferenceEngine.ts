import type { SensingFrame } from '@/types/sensing';
import { JsInferenceEngine } from './jsInferenceEngine';
import type { InferenceBackend, InferenceResult } from './types';

/**
 * WebAssembly-backed inference backend (SCAFFOLD — not yet live).
 *
 * The realistic state of WASM in React Native:
 *   - On `expo start --web` the standard `WebAssembly` global is available.
 *   - On Hermes (native iOS/Android) there is currently NO `WebAssembly`
 *     global, so a .wasm model cannot be instantiated without a native module.
 *
 * Rather than ship a fake binary, this backend:
 *   1. Probes the runtime for a usable `WebAssembly` implementation
 *      (`init()` returns false when absent — the caller then falls back to JS).
 *   2. Loads + instantiates a model via the injected {@link WasmModelLoader}
 *      when one is provided AND the runtime supports it.
 *   3. Until a real model exports an `infer` function, delegates to the JS
 *      engine so behaviour is always correct, never fabricated.
 *
 * To go live, drop in a real model: implement a loader that returns a module
 * exposing `infer(ptr,len) -> ptr` (or similar), wire {@link runModel} to it,
 * and `init()` will start returning true on supported runtimes.
 */

export interface WasmModelExports {
  /**
   * Run inference on a packed feature vector. The exact ABI is defined by the
   * model that gets dropped in; this is the seam a real model plugs into.
   */
  infer?: (...args: number[]) => number;
  memory?: WebAssembly.Memory;
}

export type WasmModelLoader = () => Promise<WasmModelExports | null>;

/** True when the current JS runtime exposes a usable WebAssembly engine. */
export function isWasmSupported(): boolean {
  return (
    typeof WebAssembly === 'object' &&
    typeof WebAssembly.instantiate === 'function' &&
    typeof WebAssembly.Module === 'function'
  );
}

export class WasmInferenceEngine implements InferenceBackend {
  readonly kind = 'wasm' as const;

  private readonly loader?: WasmModelLoader;
  private readonly fallback = new JsInferenceEngine();
  private exports: WasmModelExports | null = null;
  private modelLive = false;

  constructor(loader?: WasmModelLoader) {
    this.loader = loader;
  }

  async init(): Promise<boolean> {
    // Always init the fallback so infer() is never undefined.
    await this.fallback.init();

    if (!isWasmSupported()) {
      // No WebAssembly in this runtime (e.g. Hermes) — caller falls back to JS.
      return false;
    }

    if (!this.loader) {
      // WASM is supported but no model was supplied. We report "available" so
      // the engine can be selected, but we transparently delegate to JS until a
      // model exists. This keeps the WASM path selectable + testable today.
      return true;
    }

    try {
      this.exports = await this.loader();
      this.modelLive = typeof this.exports?.infer === 'function';
      return true;
    } catch {
      // Loader failed (bad/missing .wasm) — degrade to JS, stay usable.
      this.exports = null;
      this.modelLive = false;
      return true;
    }
  }

  infer(frame: SensingFrame): InferenceResult {
    if (this.modelLive && this.exports?.infer) {
      // SCAFFOLD: a real model would pack `frame.features` into wasm memory,
      // call `this.exports.infer(...)`, and unpack the result here. Until then
      // we never reach this branch (modelLive stays false without a real model).
      return this.runModel(frame);
    }
    // Live, correct behaviour: JS heuristics, tagged as wasm-origin engine.
    return { ...this.fallback.infer(frame), backend: this.kind };
  }

  dispose(): void {
    this.fallback.dispose();
    this.exports = null;
    this.modelLive = false;
  }

  /** Visible for testing — whether a real model is currently wired in. */
  get isModelLive(): boolean {
    return this.modelLive;
  }

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  private runModel(frame: SensingFrame): InferenceResult {
    // Placeholder for the real ABI marshalling. Falls back so this is always
    // safe to call even if a half-wired model is present.
    return { ...this.fallback.infer(frame), backend: this.kind };
  }
}
