import {
  normalizeHostToWsUrl,
  parseDetection,
  buildSimulatedSurvivors,
} from '@/services/discovery.service';

describe('normalizeHostToWsUrl', () => {
  it('adds ws:// scheme and default sensing path for host:port', () => {
    expect(normalizeHostToWsUrl('192.168.4.1:8080')).toBe('ws://192.168.4.1:8080/ws/sensing');
  });

  it('preserves an explicit path', () => {
    expect(normalizeHostToWsUrl('ws://host:9/mat')).toBe('ws://host:9/mat');
  });

  it('converts http -> ws and https -> wss', () => {
    expect(normalizeHostToWsUrl('http://host:1')).toBe('ws://host:1/ws/sensing');
    expect(normalizeHostToWsUrl('https://host:1')).toBe('wss://host:1/ws/sensing');
  });

  it('returns empty string for blank input', () => {
    expect(normalizeHostToWsUrl('   ')).toBe('');
  });
});

describe('parseDetection', () => {
  it('parses a nested coords/vitals record', () => {
    const d = parseDetection({
      id: 'x1',
      zone_id: 'A',
      coords: { x: 1, y: 2, depth: 3 },
      vitals: { breathingRate: 16, heartRate: 70, confidence: 0.8 },
    });
    expect(d).not.toBeNull();
    expect(d!.id).toBe('x1');
    expect(d!.x).toBe(1);
    expect(d!.depth).toBe(3);
    expect(d!.vitals.breathingRate).toBe(16);
    expect(d!.confidence).toBe(0.8);
  });

  it('parses snake_case vital aliases on a flat record', () => {
    const d = parseDetection({
      id: 'x2',
      x: 0,
      y: 0,
      depth: 0,
      breathing_rate: 20,
      heart_rate: 88,
      confidence: 0.5,
    });
    expect(d!.vitals.breathingRate).toBe(20);
    expect(d!.vitals.heartRate).toBe(88);
  });

  it('returns null without an id', () => {
    expect(parseDetection({ x: 1 })).toBeNull();
    expect(parseDetection(null)).toBeNull();
  });

  it('defaults missing vitals to null', () => {
    const d = parseDetection({ id: 'x3' });
    expect(d!.vitals.breathingRate).toBeNull();
    expect(d!.vitals.heartRate).toBeNull();
    expect(d!.confidence).toBe(0);
  });
});

describe('buildSimulatedSurvivors', () => {
  it('produces four survivors spanning the START categories', () => {
    const list = buildSimulatedSurvivors(1);
    expect(list).toHaveLength(4);
    const ids = list.map((s) => s.id);
    expect(ids).toEqual(['sim-A1', 'sim-B2', 'sim-C3', 'sim-D4']);
    // The BLACK survivor has no vitals.
    const black = list.find((s) => s.id === 'sim-D4')!;
    expect(black.vitals.breathingRate).toBeNull();
    expect(black.vitals.motionDetected).toBe(false);
  });
});
