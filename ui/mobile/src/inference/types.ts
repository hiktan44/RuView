import type { FeatureSet, SensingFrame } from '@/types/sensing';

/**
 * Identifies which inference backend produced (or should produce) a result.
 * - `js`:   pure-TypeScript heuristic engine (always available, no native deps)
 * - `wasm`: WebAssembly-backed model (only when a real model + runtime is present)
 */
export type InferenceBackendKind = 'js' | 'wasm';

/**
 * Where a given inference result came from in the online/offline pipeline.
 * - `server`: the result was produced by the sensing-server (frame.classification etc.)
 * - `local`:  the result was derived on-device by an {@link InferenceEngine}
 */
export type InferenceOrigin = 'server' | 'local';

export interface PresenceEstimate {
  present: boolean;
  motionLevel: 'absent' | 'present_still' | 'active';
  /** 0..1 */
  confidence: number;
  /** Best-effort person count from feature heuristics. */
  estimatedPersons: number;
}

export interface VitalsEstimate {
  /** Breaths per minute, or null when not derivable. */
  breathingBpm: number | null;
  /** Heart-rate proxy in BPM, or null when not derivable. */
  heartRateBpm: number | null;
  /** 0..1 confidence in the vital estimates. */
  confidence: number;
}

export interface InferenceResult {
  presence: PresenceEstimate;
  vitals: VitalsEstimate;
  /** Which backend computed this result. */
  backend: InferenceBackendKind;
  /** Epoch millis when this result was produced. */
  timestamp: number;
}

/**
 * A pluggable inference backend. Implementations MUST be synchronous in
 * {@link infer} (called on every frame / tick) but may do async work in
 * {@link init}. A backend is only used after {@link init} resolves `true`.
 */
export interface InferenceBackend {
  readonly kind: InferenceBackendKind;
  /**
   * Prepare the backend (e.g. load a .wasm module). Returns whether the
   * backend is usable. A `false` result signals the caller to fall back.
   */
  init(): Promise<boolean>;
  /** Derive presence + vitals from a single sensing frame. */
  infer(frame: SensingFrame): InferenceResult;
  /** Release any held resources. Safe to call multiple times. */
  dispose(): void;
}

/**
 * Window of recent feature vectors used by heuristic backends to smooth
 * single-frame noise (e.g. variance-based presence, band-power vitals).
 */
export interface FeatureWindow {
  features: FeatureSet[];
  /** Most recent first is NOT guaranteed; iterate in push order. */
  size: number;
}
