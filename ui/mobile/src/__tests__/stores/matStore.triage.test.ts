import { useMatStore, onMatRedAlert, type SurvivorDetection } from '@/stores/matStore';
import { TriageStatus, type Survivor } from '@/types/mat';

const red: SurvivorDetection = {
  id: 'r1',
  zone_id: 'A',
  x: 1,
  y: 1,
  depth: 1,
  confidence: 0.9,
  vitals: { breathingRate: 34, heartRate: 120, motionDetected: true },
};
const green: SurvivorDetection = {
  id: 'g1',
  zone_id: 'C',
  x: 2,
  y: 2,
  depth: 0.5,
  confidence: 0.9,
  vitals: { breathingRate: 15, heartRate: 72, motionDetected: true },
};
const black: SurvivorDetection = {
  id: 'b1',
  zone_id: 'D',
  x: 3,
  y: 3,
  depth: 2,
  confidence: 0.2,
  vitals: { breathingRate: null, heartRate: null, motionDetected: false },
};

const findById = (id: string): Survivor | undefined =>
  useMatStore.getState().survivors.find((s) => s.id === id);

describe('useMatStore.upsertDetection (triage)', () => {
  beforeEach(() => {
    useMatStore.setState({ events: [], zones: [], survivors: [], alerts: [], selectedEventId: null });
  });

  it('derives triage status from vitals', () => {
    useMatStore.getState().upsertDetection(red);
    useMatStore.getState().upsertDetection(green);
    useMatStore.getState().upsertDetection(black);
    expect(findById('r1')?.triage_status).toBe(TriageStatus.Immediate);
    expect(findById('g1')?.triage_status).toBe(TriageStatus.Minor);
    expect(findById('b1')?.triage_status).toBe(TriageStatus.Deceased);
  });

  it('maps vitals onto the survivor record', () => {
    useMatStore.getState().upsertDetection(green);
    const s = findById('g1')!;
    expect(s.breathing_rate).toBe(15);
    expect(s.heart_rate).toBe(72);
    expect(s.zone_id).toBe('C');
    expect(s.confidence).toBe(0.9);
  });

  it('updates an existing survivor in place (preserves first_detected)', () => {
    useMatStore.getState().upsertDetection(green);
    const first = findById('g1')!.first_detected;
    useMatStore.getState().upsertDetection({ ...green, x: 9 });
    expect(useMatStore.getState().survivors).toHaveLength(1);
    expect(findById('g1')!.x).toBe(9);
    expect(findById('g1')!.first_detected).toBe(first);
  });

  it('fires RED alert on new RED detection', () => {
    const seen: Survivor[] = [];
    const unsub = onMatRedAlert((s) => seen.push(s));
    useMatStore.getState().upsertDetection(green); // not red
    useMatStore.getState().upsertDetection(red); // red -> alert
    expect(seen).toHaveLength(1);
    expect(seen[0].id).toBe('r1');
    unsub();
  });

  it('fires RED alert on escalation to RED', () => {
    const seen: Survivor[] = [];
    const unsub = onMatRedAlert((s) => seen.push(s));
    useMatStore.getState().upsertDetection(green);
    useMatStore.getState().upsertDetection({
      ...green,
      vitals: { ...green.vitals, breathingRate: 34 },
    });
    expect(seen).toHaveLength(1);
    expect(seen[0].id).toBe('g1');
    unsub();
  });

  it('does not re-fire RED alert when already RED', () => {
    const seen: Survivor[] = [];
    const unsub = onMatRedAlert((s) => seen.push(s));
    useMatStore.getState().upsertDetection(red);
    useMatStore.getState().upsertDetection(red);
    expect(seen).toHaveLength(1);
    unsub();
  });

  it('marks deterioration when status worsens', () => {
    useMatStore.getState().upsertDetection(green); // Minor
    useMatStore.getState().upsertDetection({
      ...green,
      vitals: { breathingRate: 18, heartRate: 110, motionDetected: true }, // Delayed (worse)
    });
    expect(findById('g1')!.is_deteriorating).toBe(true);
  });
});
