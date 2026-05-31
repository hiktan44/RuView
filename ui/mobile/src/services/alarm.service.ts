import { Platform } from 'react-native';
import AsyncStorage from '@react-native-async-storage/async-storage';

/**
 * Alarm service — audible alert + haptic vibration for new/escalated RED survivors.
 *
 * Native modules (expo-haptics, expo-av) are loaded lazily and defensively:
 * if a module is missing or the platform is web, the corresponding channel
 * silently degrades. The triage data path must never crash because an alarm
 * channel was unavailable.
 *
 * Mute state is persisted via AsyncStorage so a rescuer's preference survives
 * app restarts in the field.
 */

const MUTE_STORAGE_KEY = '@ruview/alarm-muted';

/** Minimum gap between alarm fires to avoid runaway buzzing on rapid updates. */
const ALARM_THROTTLE_MS = 1500;

interface HapticsModule {
  notificationAsync: (type: unknown) => Promise<void>;
  NotificationFeedbackType: { Error: unknown; Warning: unknown };
}

interface AudioPlayer {
  play: () => void;
  remove: () => void;
}

interface AudioModule {
  createAudioPlayer: (source: unknown) => AudioPlayer;
  setAudioModeAsync?: (cfg: Record<string, unknown>) => Promise<void>;
}

/**
 * Resolve a CommonJS module by name without statically depending on `require`
 * typings (avoids @types/node). Returns null if require is absent or the
 * module cannot be loaded.
 */
function loadOptional<T>(name: string): T | null {
  try {
    const req = (globalThis as { require?: (m: string) => unknown }).require;
    if (typeof req !== 'function') {
      return null;
    }
    return req(name) as T;
  } catch {
    return null;
  }
}

class AlarmService {
  private muted = false;
  private loaded = false;
  private lastFiredAt = 0;
  private haptics: HapticsModule | null = null;
  private audio: AudioModule | null = null;

  /** Load persisted mute state + lazily resolve native modules. Idempotent. */
  async init(): Promise<void> {
    if (this.loaded) {
      return;
    }
    this.loaded = true;

    try {
      const raw = await AsyncStorage.getItem(MUTE_STORAGE_KEY);
      this.muted = raw === 'true';
    } catch {
      this.muted = false;
    }

    // Haptics + audio are native-only; web uses Vibration API + Web Audio.
    if (Platform.OS !== 'web') {
      this.haptics = loadOptional<HapticsModule>('expo-haptics');
      this.audio = loadOptional<AudioModule>('expo-audio');
    }
  }

  isMuted(): boolean {
    return this.muted;
  }

  async setMuted(muted: boolean): Promise<void> {
    this.muted = muted;
    try {
      await AsyncStorage.setItem(MUTE_STORAGE_KEY, muted ? 'true' : 'false');
    } catch {
      // Persistence failure is non-fatal.
    }
  }

  async toggleMute(): Promise<boolean> {
    await this.setMuted(!this.muted);
    return this.muted;
  }

  /**
   * Fire the alarm: haptic burst + audible tone. No-op when muted, throttled,
   * or when no channel is available. Never throws.
   */
  async fire(): Promise<void> {
    if (!this.loaded) {
      await this.init();
    }
    if (this.muted) {
      return;
    }

    const now = Date.now();
    if (now - this.lastFiredAt < ALARM_THROTTLE_MS) {
      return;
    }
    this.lastFiredAt = now;

    await Promise.all([this.vibrate(), this.beep()]);
  }

  private async vibrate(): Promise<void> {
    if (!this.haptics) {
      this.fallbackVibrate();
      return;
    }
    try {
      await this.haptics.notificationAsync(this.haptics.NotificationFeedbackType.Error);
    } catch {
      this.fallbackVibrate();
    }
  }

  private fallbackVibrate(): void {
    try {
      if (Platform.OS === 'web') {
        const nav = (globalThis as { navigator?: { vibrate?: (p: number | number[]) => boolean } })
          .navigator;
        nav?.vibrate?.([200, 100, 200]);
        return;
      }
      const rn = loadOptional<{ Vibration?: { vibrate: (p: number | number[]) => void } }>(
        'react-native',
      );
      rn?.Vibration?.vibrate([0, 200, 100, 200]);
    } catch {
      // Vibration is best-effort.
    }
  }

  private async beep(): Promise<void> {
    if (Platform.OS === 'web') {
      this.webBeep();
      return;
    }
    if (!this.audio) {
      return;
    }
    try {
      await this.audio.setAudioModeAsync?.({ playsInSilentMode: true });
      // Bundled alarm asset, resolved defensively (require at runtime) so a
      // missing file degrades to haptics/vibration only.
      const asset = this.loadAlarmAsset();
      if (asset === null) {
        return;
      }
      const player = this.audio.createAudioPlayer(asset);
      player.play();
      setTimeout(() => {
        try {
          player.remove();
        } catch {
          // ignore
        }
      }, 2500);
    } catch {
      // No asset / no audio — silent degrade.
    }
  }

  /** Resolve the bundled alarm asset, or null if it is unavailable. */
  private loadAlarmAsset(): unknown {
    try {
      const req = (globalThis as { require?: (m: string) => unknown }).require;
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      return typeof req === 'function' ? req('../../assets/alarm.wav') : null;
    } catch {
      return null;
    }
  }

  /** Web Audio API beep — no asset required, works in notebook browsers. */
  private webBeep(): void {
    try {
      const g = globalThis as {
        AudioContext?: typeof AudioContext;
        webkitAudioContext?: typeof AudioContext;
      };
      const Ctx = g.AudioContext || g.webkitAudioContext;
      if (!Ctx) {
        return;
      }
      const ctx = new Ctx();
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = 'square';
      osc.frequency.value = 880;
      gain.gain.value = 0.15;
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start();
      osc.stop(ctx.currentTime + 0.6);
      setTimeout(() => ctx.close().catch(() => undefined), 800);
    } catch {
      // Audio blocked / unavailable — ignore.
    }
  }
}

export const alarmService = new AlarmService();
