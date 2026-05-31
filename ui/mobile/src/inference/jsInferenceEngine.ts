import type { FeatureSet, SensingFrame } from '@/types/sensing';
import type { InferenceBackend, InferenceResult, PresenceEstimate, VitalsEstimate } from './types';

/**
 * Pure-TypeScript inference backend.
 *
 * This is the LIVE offline path: it has zero native dependencies and runs in
 * every RN runtime (iOS, Android, web). It derives presence + vitals purely
 * from the streamed `features` (mean RSSI, variance, motion/breathing band
 * power, spectral entropy) using deterministic heuristics, smoothed over a
 * short rolling window so a single noisy frame does not flip the output.
 *
 * It intentionally does NOT depend on `frame.classification` / `frame.vital_signs`
 * — those are the *server's* answers. When the server is gone, this engine
 * reconstructs equivalent estimates from the raw features alone, which is what
 * keeps the disaster-response use case working with the internet down.
 */

const WINDOW = 16;

// Thresholds tuned against the simulation.service feature ranges so the JS
// engine and the Rust server classifier agree closely in normal operation.
const PRESENCE_VARIANCE_THRESHOLD = 0.05;
const ACTIVE_MOTION_THRESHOLD = 0.12;
const SECOND_PERSON_VARIANCE = 0.18;
const THIRD_PERSON_VARIANCE = 0.32;

const BREATHING_BAND_MAX = 0.3;
const BREATHING_BPM_FLOOR = 8;
const BREATHING_BPM_CEIL = 24;
const HEART_BPM_FLOOR = 50;
const HEART_BPM_CEIL = 110;

const clamp = (v: number, min: number, max: number): number => Math.max(min, Math.min(max, v));

const finite = (v: number | undefined): number => (typeof v === 'number' && Number.isFinite(v) ? v : 0);

const mean = (values: number[]): number =>
  values.length === 0 ? 0 : values.reduce((sum, v) => sum + v, 0) / values.length;

export class JsInferenceEngine implements InferenceBackend {
  readonly kind = 'js' as const;

  private readonly window: FeatureSet[] = [];

  async init(): Promise<boolean> {
    return true;
  }

  infer(frame: SensingFrame): InferenceResult {
    const features = frame.features ?? ({} as FeatureSet);
    this.pushWindow(features);

    const presence = this.estimatePresence(features);
    const vitals = this.estimateVitals(features, presence.present);

    return {
      presence,
      vitals,
      backend: this.kind,
      timestamp: frame.timestamp ?? Date.now(),
    };
  }

  dispose(): void {
    this.window.length = 0;
  }

  /** Visible for testing — how many frames are currently smoothed over. */
  get windowSize(): number {
    return this.window.length;
  }

  private pushWindow(features: FeatureSet): void {
    this.window.push(features);
    if (this.window.length > WINDOW) {
      this.window.shift();
    }
  }

  private estimatePresence(latest: FeatureSet): PresenceEstimate {
    const variance = mean(this.window.map((f) => finite(f.variance)));
    const motion = mean(this.window.map((f) => finite(f.motion_band_power)));
    const entropy = finite(latest.spectral_entropy);

    const present = variance > PRESENCE_VARIANCE_THRESHOLD;
    const active = present && motion > ACTIVE_MOTION_THRESHOLD;

    const motionLevel: PresenceEstimate['motionLevel'] = active
      ? 'active'
      : present
        ? 'present_still'
        : 'absent';

    // Confidence rises with how far variance/motion clear their thresholds and
    // falls with spectral entropy (high entropy == noisier, less certain).
    const varianceMargin = clamp((variance - PRESENCE_VARIANCE_THRESHOLD) / PRESENCE_VARIANCE_THRESHOLD, 0, 1);
    const base = present ? 0.6 + varianceMargin * 0.35 : 0.5 + (1 - varianceMargin) * 0.2;
    const confidence = clamp(base * (1 - entropy * 0.2), 0, 1);

    return {
      present,
      motionLevel,
      confidence,
      estimatedPersons: this.estimatePersons(present, variance),
    };
  }

  private estimatePersons(present: boolean, variance: number): number {
    if (!present) return 0;
    if (variance > THIRD_PERSON_VARIANCE) return 3;
    if (variance > SECOND_PERSON_VARIANCE) return 2;
    return 1;
  }

  private estimateVitals(latest: FeatureSet, present: boolean): VitalsEstimate {
    if (!present) {
      return { breathingBpm: null, heartRateBpm: null, confidence: 0 };
    }

    // Prefer an explicit dominant frequency if the feature extractor provided
    // one (Hz -> BPM); otherwise map smoothed breathing-band power into a
    // plausible respiratory range.
    const breathingBand = mean(this.window.map((f) => finite(f.breathing_band_power)));
    const dominantHz = finite(latest.dominant_freq_hz);

    const breathingBpm =
      dominantHz > 0
        ? clamp(dominantHz * 60, BREATHING_BPM_FLOOR, BREATHING_BPM_CEIL)
        : clamp(
            BREATHING_BPM_FLOOR +
              (breathingBand / BREATHING_BAND_MAX) * (BREATHING_BPM_CEIL - BREATHING_BPM_FLOOR),
            BREATHING_BPM_FLOOR,
            BREATHING_BPM_CEIL,
          );

    // Heart-rate proxy: a coupled offset above breathing rate, nudged by motion
    // band power. This is a proxy (CSI cannot give true HR without a model),
    // hence the wide range and the lower confidence weighting below.
    const motion = mean(this.window.map((f) => finite(f.motion_band_power)));
    const heartRateBpm = clamp(
      HEART_BPM_FLOOR + breathingBpm * 1.8 + motion * 60,
      HEART_BPM_FLOOR,
      HEART_BPM_CEIL,
    );

    // More history + lower spectral entropy => higher confidence.
    const fill = clamp(this.window.length / WINDOW, 0, 1);
    const entropy = finite(latest.spectral_entropy);
    const confidence = clamp(0.4 + fill * 0.4 - entropy * 0.15, 0, 1);

    return { breathingBpm, heartRateBpm, confidence };
  }
}
