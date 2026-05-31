import { JsInferenceEngine } from '@/inference/jsInferenceEngine';
import type { FeatureSet, SensingFrame } from '@/types/sensing';

const makeFrame = (features: Partial<FeatureSet>, timestamp = 1000): SensingFrame => ({
  type: 'sensing_update',
  timestamp,
  nodes: [],
  features: {
    mean_rssi: -45,
    variance: 0,
    motion_band_power: 0,
    breathing_band_power: 0,
    spectral_entropy: 0,
    ...features,
  },
  classification: { motion_level: 'absent', presence: false, confidence: 0 },
  signal_field: { grid_size: [20, 1, 20], values: [] },
});

describe('JsInferenceEngine', () => {
  it('init resolves true (always available)', async () => {
    const engine = new JsInferenceEngine();
    await expect(engine.init()).resolves.toBe(true);
  });

  it('reports absent presence for low-variance frames', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    const result = engine.infer(makeFrame({ variance: 0.01 }));
    expect(result.presence.present).toBe(false);
    expect(result.presence.motionLevel).toBe('absent');
    expect(result.presence.estimatedPersons).toBe(0);
  });

  it('detects present_still when variance clears threshold but motion is low', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    const result = engine.infer(makeFrame({ variance: 0.1, motion_band_power: 0.01 }));
    expect(result.presence.present).toBe(true);
    expect(result.presence.motionLevel).toBe('present_still');
    expect(result.presence.estimatedPersons).toBeGreaterThanOrEqual(1);
  });

  it('detects active motion when motion band power is high', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    const result = engine.infer(makeFrame({ variance: 0.1, motion_band_power: 0.3 }));
    expect(result.presence.motionLevel).toBe('active');
  });

  it('scales estimated persons with variance', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    expect(engine.infer(makeFrame({ variance: 0.1 })).presence.estimatedPersons).toBe(1);
    engine.dispose();
    await engine.init();
    expect(engine.infer(makeFrame({ variance: 0.2 })).presence.estimatedPersons).toBe(2);
    engine.dispose();
    await engine.init();
    expect(engine.infer(makeFrame({ variance: 0.4 })).presence.estimatedPersons).toBe(3);
  });

  it('returns null vitals when no one is present', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    const result = engine.infer(makeFrame({ variance: 0.001 }));
    expect(result.vitals.breathingBpm).toBeNull();
    expect(result.vitals.heartRateBpm).toBeNull();
    expect(result.vitals.confidence).toBe(0);
  });

  it('derives breathing bpm from dominant frequency when present', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    const result = engine.infer(makeFrame({ variance: 0.1, dominant_freq_hz: 0.3 }));
    // 0.3 Hz -> 18 BPM, within physiological clamp
    expect(result.vitals.breathingBpm).toBeCloseTo(18, 0);
    expect(result.vitals.heartRateBpm).not.toBeNull();
  });

  it('clamps breathing bpm into a physiological range', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    const result = engine.infer(makeFrame({ variance: 0.1, dominant_freq_hz: 5 }));
    expect(result.vitals.breathingBpm!).toBeLessThanOrEqual(24);
    expect(result.vitals.breathingBpm!).toBeGreaterThanOrEqual(8);
  });

  it('smooths over a rolling window (windowSize grows then caps)', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    for (let i = 0; i < 5; i += 1) {
      engine.infer(makeFrame({ variance: 0.1 }, 1000 + i));
    }
    expect(engine.windowSize).toBe(5);
    for (let i = 0; i < 50; i += 1) {
      engine.infer(makeFrame({ variance: 0.1 }, 2000 + i));
    }
    expect(engine.windowSize).toBeLessThanOrEqual(16);
  });

  it('confidence increases as the window fills', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    const first = engine.infer(makeFrame({ variance: 0.1, dominant_freq_hz: 0.3 }));
    for (let i = 0; i < 16; i += 1) {
      engine.infer(makeFrame({ variance: 0.1, dominant_freq_hz: 0.3 }, 1000 + i));
    }
    const later = engine.infer(makeFrame({ variance: 0.1, dominant_freq_hz: 0.3 }, 2000));
    expect(later.vitals.confidence).toBeGreaterThanOrEqual(first.vitals.confidence);
  });

  it('tags the backend as js and carries frame timestamp', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    const result = engine.infer(makeFrame({ variance: 0.1 }, 4242));
    expect(result.backend).toBe('js');
    expect(result.timestamp).toBe(4242);
  });

  it('tolerates missing/NaN features without throwing', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    const frame = makeFrame({});
    // @ts-expect-error intentionally corrupt to test robustness
    frame.features = { variance: NaN, motion_band_power: undefined };
    expect(() => engine.infer(frame)).not.toThrow();
  });

  it('dispose clears the window', async () => {
    const engine = new JsInferenceEngine();
    await engine.init();
    engine.infer(makeFrame({ variance: 0.1 }));
    engine.dispose();
    expect(engine.windowSize).toBe(0);
  });
});
