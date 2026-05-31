import { wsService } from '@/services/ws.service';
import { RingBuffer } from '@/utils/ringBuffer';
import { createInferenceEngine, type EnginePreference } from '@/inference';
import type { InferenceBackend, InferenceOrigin, InferenceResult } from '@/inference';
import type { ConnectionStatus, SensingFrame } from '@/types/sensing';

/**
 * CSI / sensing streaming service with offline-capable local inference.
 *
 * Online -> Offline failover (the disaster-response core of this feature):
 *   - While the sensing-server WebSocket is `connected`, frames carry the
 *     server's own classification + vitals. We surface those as origin
 *     `'server'` (unless the user forces local inference).
 *   - When the WS drops (status becomes `simulated`/`disconnected`), we keep
 *     producing presence + vital estimates ON-DEVICE from the most recent
 *     buffered CSI features, on a fixed tick, via the {@link InferenceBackend}.
 *     The phone keeps working with the internet down.
 *
 * Frame buffering reuses the shared {@link RingBuffer} util.
 */

export interface SensingStreamUpdate {
  frame: SensingFrame | null;
  result: InferenceResult | null;
  origin: InferenceOrigin;
  /** True when results are being produced locally (offline or forced). */
  offline: boolean;
  connectionStatus: ConnectionStatus;
}

type StreamListener = (update: SensingStreamUpdate) => void;

const FRAME_BUFFER_SIZE = 120;
/** How often to re-run local inference when offline with no fresh frames. */
const OFFLINE_TICK_MS = 1000;

const isServerLive = (status: ConnectionStatus): boolean => status === 'connected';

export class CsiStreamService {
  private readonly buffer = new RingBuffer<SensingFrame>(FRAME_BUFFER_SIZE);
  private readonly listeners = new Set<StreamListener>();

  private engine: InferenceBackend | null = null;
  private enginePref: EnginePreference = 'auto';
  private forceLocal = false;

  private status: ConnectionStatus = 'disconnected';
  private lastFrame: SensingFrame | null = null;
  private lastResult: InferenceResult | null = null;
  private offlineTimer: ReturnType<typeof setInterval> | null = null;
  private unsubscribeWs: (() => void) | null = null;

  /**
   * Start consuming frames from the WS service and producing inference.
   * Idempotent: re-calling with the same config is a no-op beyond re-init.
   */
  async start(config: { enginePreference?: EnginePreference; forceLocal?: boolean } = {}): Promise<void> {
    this.enginePref = config.enginePreference ?? 'auto';
    this.forceLocal = config.forceLocal ?? false;

    await this.ensureEngine();

    if (!this.unsubscribeWs) {
      this.unsubscribeWs = wsService.subscribe((frame) => this.onFrame(frame));
    }
    this.status = wsService.getStatus();
    this.syncOfflineTimer();
  }

  stop(): void {
    this.unsubscribeWs?.();
    this.unsubscribeWs = null;
    this.clearOfflineTimer();
    this.engine?.dispose();
    this.engine = null;
    this.buffer.clear();
    this.lastFrame = null;
    this.lastResult = null;
  }

  subscribe(listener: StreamListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /** Called by external connection tracking (e.g. poseStore) when status changes. */
  setConnectionStatus(status: ConnectionStatus): void {
    if (status === this.status) return;
    this.status = status;
    this.syncOfflineTimer();
    this.emit();
  }

  setForceLocal(value: boolean): void {
    if (value === this.forceLocal) return;
    this.forceLocal = value;
    this.syncOfflineTimer();
    this.emit();
  }

  async setEnginePreference(pref: EnginePreference): Promise<void> {
    if (pref === this.enginePref && this.engine) return;
    this.enginePref = pref;
    this.engine?.dispose();
    this.engine = null;
    await this.ensureEngine();
    this.emit();
  }

  getSnapshot(): SensingStreamUpdate {
    return {
      frame: this.lastFrame,
      result: this.lastResult,
      origin: this.currentOrigin(),
      offline: this.isOffline(),
      connectionStatus: this.status,
    };
  }

  /** Visible for testing — current buffered frame count. */
  get bufferedCount(): number {
    return this.buffer.toArray().length;
  }

  private async ensureEngine(): Promise<void> {
    if (this.engine) return;
    const engine = createInferenceEngine({ preference: this.enginePref });
    await engine.init();
    this.engine = engine;
  }

  private onFrame(frame: SensingFrame): void {
    this.buffer.push(frame);
    this.lastFrame = frame;
    // Track status from the frame source as a hint (ws.service sets store too).
    if (frame.source === 'simulated' && this.status === 'connected') {
      this.status = 'simulated';
      this.syncOfflineTimer();
    }
    this.runInference(frame);
    this.emit();
  }

  private runInference(frame: SensingFrame): void {
    if (!this.engine) return;
    this.lastResult = this.engine.infer(frame);
  }

  private isOffline(): boolean {
    return this.forceLocal || !isServerLive(this.status);
  }

  private currentOrigin(): InferenceOrigin {
    // When the server is live and the user hasn't forced local inference, the
    // authoritative answer is the server's; otherwise we are running locally.
    return this.isOffline() ? 'local' : 'server';
  }

  private syncOfflineTimer(): void {
    if (this.isOffline()) {
      this.startOfflineTimer();
    } else {
      this.clearOfflineTimer();
    }
  }

  private startOfflineTimer(): void {
    if (this.offlineTimer) return;
    this.offlineTimer = setInterval(() => {
      // Re-run inference on the last buffered frame so estimates keep flowing
      // even if no new frames arrive (e.g. ESP32 link briefly silent).
      if (this.lastFrame) {
        this.runInference(this.lastFrame);
        this.emit();
      }
    }, OFFLINE_TICK_MS);
  }

  private clearOfflineTimer(): void {
    if (this.offlineTimer) {
      clearInterval(this.offlineTimer);
      this.offlineTimer = null;
    }
  }

  private emit(): void {
    const update = this.getSnapshot();
    this.listeners.forEach((listener) => listener(update));
  }
}

export const csiStreamService = new CsiStreamService();
