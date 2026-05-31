import { TriageStatus } from '@/types/mat';

/**
 * START triage logic for the WiFi-MAT disaster-response module.
 *
 * Maps WiFi-sensed vital signs (breathing + heart rate) to a START
 * (Simple Triage And Rapid Treatment) category per the RuView PRD.
 */

/** Vital signs extracted from WiFi sensing for one detected person. */
export interface TriageVitals {
  /** Respiration rate in breaths per minute, or null if undetectable. */
  breathingRate: number | null;
  /** Heart rate in beats per minute, or null if undetectable. */
  heartRate: number | null;
  /** Pulse signal is weak / irregular (low SNR or arrhythmic). */
  irregularPulse?: boolean;
  /** Any micro-motion detected (chest rise, limb movement). */
  motionDetected?: boolean;
}

/** START thresholds (breaths per minute). */
export const TRIAGE_THRESHOLDS = {
  respHigh: 30,
  respLow: 10,
  /** Calm, healthy respiration band used to qualify GREEN/Minor. */
  respCalmMin: 12,
  respCalmMax: 20,
  /** Resting heart-rate band used to qualify GREEN/Minor (strong, steady pulse). */
  hrStrongMin: 55,
  hrStrongMax: 100,
} as const;

/** Lower number = higher rescue priority. Mirrors the enum ordering. */
export const TRIAGE_PRIORITY: Record<TriageStatus, number> = {
  [TriageStatus.Immediate]: 0,
  [TriageStatus.Delayed]: 1,
  [TriageStatus.Minor]: 2,
  [TriageStatus.Deceased]: 3,
  [TriageStatus.Unknown]: 4,
};

/** START colour labels (RED / YELLOW / GREEN / BLACK) per category. */
export const TRIAGE_META: Record<TriageStatus, { label: string; short: string; colorKey: string }> = {
  [TriageStatus.Immediate]: { label: 'IMMEDIATE', short: 'RED', colorKey: 'danger' },
  [TriageStatus.Delayed]: { label: 'DELAYED', short: 'YELLOW', colorKey: 'warn' },
  [TriageStatus.Minor]: { label: 'MINOR', short: 'GREEN', colorKey: 'success' },
  [TriageStatus.Deceased]: { label: 'DECEASED', short: 'BLACK', colorKey: 'muted' },
  [TriageStatus.Unknown]: { label: 'UNKNOWN', short: 'GREY', colorKey: 'textSecondary' },
};

/** High-contrast hex colours (sunlight-readable) for each START category. */
export const TRIAGE_COLOR: Record<TriageStatus, { bg: string; fg: string }> = {
  [TriageStatus.Immediate]: { bg: '#DC2626', fg: '#FFFFFF' },
  [TriageStatus.Delayed]: { bg: '#FACC15', fg: '#1A1400' },
  [TriageStatus.Minor]: { bg: '#16A34A', fg: '#FFFFFF' },
  [TriageStatus.Deceased]: { bg: '#0B0B0B', fg: '#E5E5E5' },
  [TriageStatus.Unknown]: { bg: '#475569', fg: '#FFFFFF' },
};

/**
 * Pure mapping from sensed vitals to a START triage status.
 *
 * Evaluation order matters: BLACK (no life signs) and RED (critical) take
 * precedence over GREEN/YELLOW so a borderline case is never under-triaged.
 *
 * START mapping per PRD:
 *  - RED (Immediate): respiration > 30 or < 10 /min, OR weak/irregular pulse
 *  - YELLOW (Delayed): stable respiration (10..30) + regular pulse
 *  - GREEN (Minor): very strong vitals (calm respiration + steady strong pulse)
 *  - BLACK (Deceased): no motion AND no breath detected
 *
 * @example
 * computeTriage({ breathingRate: 8, heartRate: 70 }); // Immediate (RED)
 * computeTriage({ breathingRate: null, heartRate: null, motionDetected: false }); // Deceased (BLACK)
 */
export function computeTriage(vitals: TriageVitals): TriageStatus {
  const { breathingRate, heartRate, irregularPulse, motionDetected } = vitals;

  const noBreath = breathingRate === null || breathingRate <= 0;
  const noPulse = heartRate === null || heartRate <= 0;
  const noMotion = motionDetected !== true;

  // BLACK — no detectable life signs at all.
  if (noBreath && noPulse && noMotion) {
    return TriageStatus.Deceased;
  }

  // RED — critical respiration extremes or a weak/irregular pulse.
  if (
    (breathingRate !== null &&
      (breathingRate > TRIAGE_THRESHOLDS.respHigh || breathingRate < TRIAGE_THRESHOLDS.respLow)) ||
    irregularPulse === true ||
    // Breathing but no measurable pulse is still immediate.
    (!noBreath && noPulse)
  ) {
    return TriageStatus.Immediate;
  }

  // From here respiration is within [respLow, respHigh] and pulse is regular.

  // GREEN — very strong, calm vitals.
  if (
    breathingRate !== null &&
    heartRate !== null &&
    breathingRate >= TRIAGE_THRESHOLDS.respCalmMin &&
    breathingRate <= TRIAGE_THRESHOLDS.respCalmMax &&
    heartRate >= TRIAGE_THRESHOLDS.hrStrongMin &&
    heartRate <= TRIAGE_THRESHOLDS.hrStrongMax
  ) {
    return TriageStatus.Minor;
  }

  // YELLOW — stable respiration + regular pulse, but not "very strong".
  return TriageStatus.Delayed;
}
