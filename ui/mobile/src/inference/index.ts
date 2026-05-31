import { JsInferenceEngine } from './jsInferenceEngine';
import { WasmInferenceEngine, isWasmSupported, type WasmModelLoader } from './wasmInferenceEngine';
import type { InferenceBackend, InferenceBackendKind } from './types';

export * from './types';
export { JsInferenceEngine } from './jsInferenceEngine';
export { WasmInferenceEngine, isWasmSupported } from './wasmInferenceEngine';
export type { WasmModelLoader, WasmModelExports } from './wasmInferenceEngine';

export type EnginePreference = InferenceBackendKind | 'auto';

export interface CreateEngineOptions {
  preference?: EnginePreference;
  wasmLoader?: WasmModelLoader;
}

/**
 * Construct an inference backend.
 *
 * - `'js'`   -> always the JS heuristic engine.
 * - `'wasm'` -> WASM engine if supported, else JS.
 * - `'auto'` -> WASM when the runtime supports it (with a model loader), else JS.
 *
 * The returned backend is NOT yet initialised; call `await engine.init()`.
 */
export function createInferenceEngine(options: CreateEngineOptions = {}): InferenceBackend {
  const { preference = 'auto', wasmLoader } = options;

  if (preference === 'js') {
    return new JsInferenceEngine();
  }

  if (preference === 'wasm') {
    return new WasmInferenceEngine(wasmLoader);
  }

  // auto: prefer WASM only when the runtime can actually run it.
  return isWasmSupported() ? new WasmInferenceEngine(wasmLoader) : new JsInferenceEngine();
}
