import AsyncStorage from '@react-native-async-storage/async-storage';
import { RECONNECT_DELAYS, MAX_RECONNECT_ATTEMPTS } from '@/constants/websocket';
import { useMatStore, type SurvivorDetection } from '@/stores/matStore';
import { usePoseStore } from '@/stores/poseStore';
import { generateSimulatedData } from '@/services/simulation.service';

/**
 * Discovery / connection service for the field rescuer.
 *
 * - Lets the rescuer point the app at a sensing-server host:port.
 * - Persists recently-used hosts (most-recent first) via AsyncStorage.
 * - Connects via WebSocket with exponential backoff (same delays as ws.service).
 * - Feeds incoming sensing/MAT messages into the triage store as detections.
 * - Falls back to a simulated survivor stream when the server is unreachable,
 *   so the UI stays demoable offline.
 */

const RECENT_HOSTS_KEY = '@ruview/recent-hosts';
const MAX_RECENT_HOSTS = 5;
const SIM_TICK_MS = 1500;

function toNumberOrNull(v: unknown): number | null {
  return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

/** Normalise a free-form host into a ws:// MAT endpoint URL. */
export function normalizeHostToWsUrl(host: string): string {
  const trimmed = host.trim();
  if (!trimmed) {
    return '';
  }
  let url = trimmed;
  if (!/^wss?:\/\//i.test(url) && !/^https?:\/\//i.test(url)) {
    url = `ws://${url}`;
  }
  url = url.replace(/^http:/i, 'ws:').replace(/^https:/i, 'wss:');
  try {
    const u = new URL(url);
    if (!u.pathname || u.pathname === '/') {
      u.pathname = '/ws/sensing';
    }
    return u.toString();
  } catch {
    return `${url.replace(/\/$/, '')}/ws/sensing`;
  }
}

/** Parse one record from a sensing/MAT message into a SurvivorDetection. */
export function parseDetection(raw: unknown): SurvivorDetection | null {
  if (!raw || typeof raw !== 'object') {
    return null;
  }
  const r = raw as Record<string, unknown>;
  const id = typeof r.id === 'string' ? r.id : r.id != null ? String(r.id) : '';
  if (!id) {
    return null;
  }

  const coords = (r.coords ?? r) as Record<string, unknown>;
  const vitalsRaw = (r.vitals ?? r) as Record<string, unknown>;

  return {
    id,
    zone_id: typeof r.zone_id === 'string' ? r.zone_id : undefined,
    x: toNumberOrNull(coords.x) ?? 0,
    y: toNumberOrNull(coords.y) ?? 0,
    depth: toNumberOrNull(coords.depth) ?? 0,
    confidence: toNumberOrNull(vitalsRaw.confidence ?? r.confidence) ?? 0,
    vitals: {
      breathingRate: toNumberOrNull(
        vitalsRaw.breathingRate ?? vitalsRaw.breathing_rate ?? vitalsRaw.breathing_bpm,
      ),
      heartRate: toNumberOrNull(
        vitalsRaw.heartRate ?? vitalsRaw.heart_rate ?? vitalsRaw.hr_proxy_bpm,
      ),
      irregularPulse: vitalsRaw.irregularPulse === true || vitalsRaw.irregular_pulse === true,
      motionDetected: vitalsRaw.motionDetected === true || vitalsRaw.motion_detected === true,
    },
  };
}

/** Apply a parsed sensing/MAT message to the triage store. */
export function applyMatMessage(data: unknown): void {
  if (!data || typeof data !== 'object') {
    return;
  }
  const msg = data as { survivors?: unknown; detections?: unknown };
  const list = msg.survivors ?? msg.detections;
  if (!Array.isArray(list)) {
    return;
  }
  const upsert = useMatStore.getState().upsertDetection;
  for (const item of list) {
    const parsed = parseDetection(item);
    if (parsed) {
      upsert(parsed);
    }
  }
}

export async function getRecentHosts(): Promise<string[]> {
  try {
    const raw = await AsyncStorage.getItem(RECENT_HOSTS_KEY);
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed) ? parsed.filter((h): h is string => typeof h === 'string') : [];
  } catch {
    return [];
  }
}

export async function rememberHost(host: string): Promise<string[]> {
  const clean = host.trim();
  if (!clean) {
    return getRecentHosts();
  }
  try {
    const current = await getRecentHosts();
    const next = [clean, ...current.filter((h) => h !== clean)].slice(0, MAX_RECENT_HOSTS);
    await AsyncStorage.setItem(RECENT_HOSTS_KEY, JSON.stringify(next));
    return next;
  } catch {
    return getRecentHosts();
  }
}

/**
 * Build a simulated survivor batch covering all four START categories with
 * jittering vitals, so the triage UI (and alarm) are demoable offline.
 */
export function buildSimulatedSurvivors(tick: number): SurvivorDetection[] {
  const jitter = (v: number, amt: number) => Math.max(0, Math.round(v + (Math.random() - 0.5) * amt));
  return [
    {
      id: 'sim-A1',
      zone_id: 'Sector A',
      x: 2.1,
      y: 1.4,
      depth: 1.2,
      confidence: 0.6 + Math.random() * 0.35,
      vitals: { breathingRate: jitter(34, 4), heartRate: jitter(120, 8), irregularPulse: tick % 2 === 0, motionDetected: true },
    },
    {
      id: 'sim-B2',
      zone_id: 'Sector B',
      x: 5.6,
      y: 3.2,
      depth: 0.6,
      confidence: 0.55 + Math.random() * 0.4,
      vitals: { breathingRate: jitter(18, 3), heartRate: jitter(92, 6), motionDetected: true },
    },
    {
      id: 'sim-C3',
      zone_id: 'Sector C',
      x: 8.0,
      y: 6.1,
      depth: 0.3,
      confidence: 0.7 + Math.random() * 0.25,
      vitals: { breathingRate: jitter(15, 2), heartRate: jitter(72, 4), motionDetected: true },
    },
    {
      id: 'sim-D4',
      zone_id: 'Sector D',
      x: 3.3,
      y: 7.5,
      depth: 2.4,
      confidence: 0.3 + Math.random() * 0.2,
      vitals: { breathingRate: null, heartRate: null, motionDetected: false },
    },
  ];
}

/**
 * Manages a single live connection lifecycle: connect to a host, feed the
 * triage store, and drop to simulation if the server is unreachable.
 */
export class DiscoveryService {
  private ws: WebSocket | null = null;
  private simTimer: ReturnType<typeof setInterval> | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempt = 0;
  private targetUrl = '';
  private active = false;
  private simTick = 0;

  /** Connect to a host (free-form host:port or full ws/http URL). */
  async connect(host: string): Promise<void> {
    this.disconnect();
    await rememberHost(host);

    this.targetUrl = normalizeHostToWsUrl(host);
    this.active = true;
    this.reconnectAttempt = 0;

    if (!this.targetUrl) {
      this.startSimulation();
      return;
    }
    this.openSocket();
  }

  private openSocket(): void {
    usePoseStore.getState().setConnectionStatus('connecting');
    try {
      const socket = new WebSocket(this.targetUrl);
      this.ws = socket;

      socket.onopen = () => {
        this.reconnectAttempt = 0;
        this.stopSimulation();
        usePoseStore.getState().setConnectionStatus('connected');
      };

      socket.onmessage = (evt) => {
        try {
          const raw = typeof evt.data === 'string' ? evt.data : JSON.stringify(evt.data);
          applyMatMessage(JSON.parse(raw));
        } catch {
          // ignore malformed frames
        }
      };

      socket.onclose = (evt) => {
        this.ws = null;
        if (!this.active || evt?.code === 1000) {
          usePoseStore.getState().setConnectionStatus('disconnected');
          return;
        }
        this.scheduleReconnect();
      };

      socket.onerror = () => {
        // handled by onclose
      };
    } catch {
      this.scheduleReconnect();
    }
  }

  private scheduleReconnect(): void {
    if (!this.active) {
      usePoseStore.getState().setConnectionStatus('disconnected');
      return;
    }
    // Once attempts are exhausted, fall back to simulation so the UI stays usable.
    if (this.reconnectAttempt >= MAX_RECONNECT_ATTEMPTS) {
      this.startSimulation();
      return;
    }
    const delay = RECONNECT_DELAYS[Math.min(this.reconnectAttempt, RECONNECT_DELAYS.length - 1)];
    this.reconnectAttempt += 1;
    this.clearReconnectTimer();
    // Show simulation while reconnecting so the rescuer always sees data.
    this.startSimulation();
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.openSocket();
    }, delay);
  }

  /** Start the offline demo stream immediately. */
  startSimulation(): void {
    if (this.simTimer) {
      return;
    }
    usePoseStore.getState().setConnectionStatus('simulated');
    this.simTimer = setInterval(() => {
      this.simTick += 1;
      applyMatMessage({ survivors: buildSimulatedSurvivors(this.simTick) });
      // Keep pose store fed too, so other screens animate.
      usePoseStore.getState().handleFrame(generateSimulatedData());
    }, SIM_TICK_MS);
    // Emit one batch immediately.
    applyMatMessage({ survivors: buildSimulatedSurvivors(this.simTick) });
  }

  private stopSimulation(): void {
    if (this.simTimer) {
      clearInterval(this.simTimer);
      this.simTimer = null;
    }
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  disconnect(): void {
    this.active = false;
    this.clearReconnectTimer();
    this.stopSimulation();
    if (this.ws) {
      try {
        this.ws.close(1000, 'client disconnect');
      } catch {
        // ignore
      }
      this.ws = null;
    }
  }
}

export const discoveryService = new DiscoveryService();
