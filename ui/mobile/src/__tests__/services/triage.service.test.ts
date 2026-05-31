import { computeTriage, type TriageVitals } from '@/services/triage.service';
import { TriageStatus } from '@/types/mat';

const base: TriageVitals = {
  breathingRate: 16,
  heartRate: 75,
  irregularPulse: false,
  motionDetected: true,
};

describe('computeTriage', () => {
  it('returns BLACK (Deceased) when no breath, no pulse, no motion', () => {
    expect(
      computeTriage({ breathingRate: null, heartRate: null, motionDetected: false }),
    ).toBe(TriageStatus.Deceased);
    expect(
      computeTriage({ breathingRate: 0, heartRate: 0, motionDetected: false }),
    ).toBe(TriageStatus.Deceased);
  });

  it('returns RED (Immediate) when respiration is too high (>30)', () => {
    expect(computeTriage({ ...base, breathingRate: 34 })).toBe(TriageStatus.Immediate);
  });

  it('returns RED (Immediate) when respiration is too low (<10)', () => {
    expect(computeTriage({ ...base, breathingRate: 8 })).toBe(TriageStatus.Immediate);
  });

  it('returns RED (Immediate) when pulse is weak/irregular', () => {
    expect(computeTriage({ ...base, irregularPulse: true })).toBe(TriageStatus.Immediate);
  });

  it('returns RED (Immediate) when breathing present but no measurable pulse', () => {
    expect(computeTriage({ ...base, heartRate: null })).toBe(TriageStatus.Immediate);
  });

  it('returns GREEN (Minor) for very strong calm vitals', () => {
    expect(computeTriage({ ...base, breathingRate: 15, heartRate: 72 })).toBe(TriageStatus.Minor);
  });

  it('returns YELLOW (Delayed) for stable but non-ideal vitals', () => {
    // Respiration in range but heart rate above the strong band -> not GREEN.
    expect(computeTriage({ ...base, breathingRate: 18, heartRate: 110 })).toBe(TriageStatus.Delayed);
    // Respiration inside range but outside calm band -> YELLOW.
    expect(computeTriage({ ...base, breathingRate: 28, heartRate: 80 })).toBe(TriageStatus.Delayed);
  });

  it('handles boundaries: 10 /min is YELLOW, 9 /min is RED', () => {
    expect(computeTriage({ ...base, breathingRate: 10, heartRate: 80 })).toBe(TriageStatus.Delayed);
    expect(computeTriage({ ...base, breathingRate: 9, heartRate: 80 })).toBe(TriageStatus.Immediate);
  });

  it('survivor with motion but no measurable vitals is not BLACK', () => {
    expect(
      computeTriage({ breathingRate: null, heartRate: null, motionDetected: true }),
    ).not.toBe(TriageStatus.Deceased);
  });
});
