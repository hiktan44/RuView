import { create } from 'zustand';
import type { Alert, DisasterEvent, ScanZone, Survivor } from '@/types/mat';
import { TriageStatus } from '@/types/mat';
import { computeTriage, TRIAGE_COLOR, type TriageVitals } from '@/services/triage.service';

/**
 * Minimal sensing-derived detection used by the field triage feed. Triage
 * status is derived from vitals via {@link computeTriage}; callers should not
 * trust an externally-provided status.
 */
export interface SurvivorDetection {
  id: string;
  zone_id?: string;
  x: number;
  y: number;
  depth: number;
  vitals: TriageVitals;
  confidence: number;
}

/** Fired when a survivor newly becomes RED (new detection or escalation). */
export type RedAlertListener = (survivor: Survivor) => void;

const redAlertListeners = new Set<RedAlertListener>();

/** Subscribe to RED-detection / escalation events (drives the alarm). */
export function onMatRedAlert(listener: RedAlertListener): () => void {
  redAlertListeners.add(listener);
  return () => {
    redAlertListeners.delete(listener);
  };
}

function notifyRedAlert(survivor: Survivor): void {
  redAlertListeners.forEach((listener) => {
    try {
      listener(survivor);
    } catch {
      // Never let an alarm sink crash the data path.
    }
  });
}

export interface MatState {
  events: DisasterEvent[];
  zones: ScanZone[];
  survivors: Survivor[];
  alerts: Alert[];
  selectedEventId: string | null;
  upsertEvent: (event: DisasterEvent) => void;
  addZone: (zone: ScanZone) => void;
  upsertSurvivor: (survivor: Survivor) => void;
  /** Insert/update a survivor from sensing vitals; derives START triage status. */
  upsertDetection: (detection: SurvivorDetection) => void;
  addAlert: (alert: Alert) => void;
  setSelectedEvent: (id: string | null) => void;
}

export const useMatStore = create<MatState>((set) => ({
  events: [],
  zones: [],
  survivors: [],
  alerts: [],
  selectedEventId: null,

  upsertEvent: (event) => {
    set((state) => {
      const index = state.events.findIndex((item) => item.event_id === event.event_id);
      if (index === -1) {
        return { events: [...state.events, event] };
      }
      const events = [...state.events];
      events[index] = event;
      return { events };
    });
  },

  addZone: (zone) => {
    set((state) => {
      const index = state.zones.findIndex((item) => item.id === zone.id);
      if (index === -1) {
        return { zones: [...state.zones, zone] };
      }
      const zones = [...state.zones];
      zones[index] = zone;
      return { zones };
    });
  },

  upsertSurvivor: (survivor) => {
    set((state) => {
      const index = state.survivors.findIndex((item) => item.id === survivor.id);
      if (index === -1) {
        return { survivors: [...state.survivors, survivor] };
      }
      const survivors = [...state.survivors];
      survivors[index] = survivor;
      return { survivors };
    });
  },

  upsertDetection: (detection) => {
    const nowIso = new Date().toISOString();
    const status = computeTriage(detection.vitals);
    const result: { escalatedToRed: boolean; survivor: Survivor | undefined } = {
      escalatedToRed: false,
      survivor: undefined,
    };

    set((state) => {
      const existing = state.survivors.find((item) => item.id === detection.id);
      const wasRed = existing?.triage_status === TriageStatus.Immediate;

      const survivor: Survivor = {
        id: detection.id,
        zone_id: detection.zone_id ?? existing?.zone_id ?? 'unknown',
        x: detection.x,
        y: detection.y,
        depth: detection.depth,
        triage_status: status,
        triage_color: TRIAGE_COLOR[status].bg,
        confidence: detection.confidence,
        breathing_rate: detection.vitals.breathingRate ?? 0,
        heart_rate: detection.vitals.heartRate ?? 0,
        first_detected: existing?.first_detected ?? nowIso,
        last_updated: nowIso,
        is_deteriorating: existing
          ? status < existing.triage_status // lower enum = more severe
          : false,
      };

      result.escalatedToRed = status === TriageStatus.Immediate && !wasRed;
      result.survivor = survivor;

      if (!existing) {
        return { survivors: [...state.survivors, survivor] };
      }
      const survivors = state.survivors.map((item) =>
        item.id === survivor.id ? survivor : item,
      );
      return { survivors };
    });

    if (result.escalatedToRed && result.survivor) {
      notifyRedAlert(result.survivor);
    }
  },

  addAlert: (alert) => {
    set((state) => {
      if (state.alerts.some((item) => item.id === alert.id)) {
        return {
          alerts: state.alerts.map((item) => (item.id === alert.id ? alert : item)),
        };
      }
      return { alerts: [...state.alerts, alert] };
    });
  },

  setSelectedEvent: (id) => {
    set({ selectedEventId: id });
  },
}));
