import type { ConnectionStatus, FeatureSet, SensingFrame } from '@/types/sensing';

// Mock the WS service so we can drive frames + control reported status.
let frameCallback: ((frame: SensingFrame) => void) | null = null;
let mockStatus: ConnectionStatus = 'connected';

jest.mock('@/services/ws.service', () => ({
  wsService: {
    subscribe: jest.fn((cb: (f: SensingFrame) => void) => {
      frameCallback = cb;
      return () => {
        frameCallback = null;
      };
    }),
    getStatus: jest.fn(() => mockStatus),
    connect: jest.fn(),
    disconnect: jest.fn(),
  },
}));

import { CsiStreamService } from '@/services/csiStream.service';

const makeFrame = (
  features: Partial<FeatureSet>,
  source?: string,
  timestamp = 1000,
): SensingFrame => ({
  type: 'sensing_update',
  timestamp,
  source,
  nodes: [],
  features: {
    mean_rssi: -45,
    variance: 0.1,
    motion_band_power: 0.1,
    breathing_band_power: 0.1,
    spectral_entropy: 0,
    ...features,
  },
  classification: { motion_level: 'absent', presence: false, confidence: 0 },
  signal_field: { grid_size: [20, 1, 20], values: [] },
});

describe('CsiStreamService', () => {
  beforeEach(() => {
    jest.useFakeTimers();
    frameCallback = null;
    mockStatus = 'connected';
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  it('subscribes to the ws service on start', async () => {
    const service = new CsiStreamService();
    await service.start({ enginePreference: 'js' });
    expect(frameCallback).toBeInstanceOf(Function);
    service.stop();
  });

  it('buffers incoming frames in the ring buffer', async () => {
    const service = new CsiStreamService();
    await service.start({ enginePreference: 'js' });
    frameCallback!(makeFrame({ variance: 0.1 }));
    frameCallback!(makeFrame({ variance: 0.1 }));
    expect(service.bufferedCount).toBe(2);
    service.stop();
  });

  it('reports origin "server" while connected and not forced', async () => {
    const service = new CsiStreamService();
    await service.start({ enginePreference: 'js' });
    service.setConnectionStatus('connected');
    frameCallback!(makeFrame({ variance: 0.1 }));
    const snap = service.getSnapshot();
    expect(snap.origin).toBe('server');
    expect(snap.offline).toBe(false);
    service.stop();
  });

  it('switches to local inference when the connection drops', async () => {
    const service = new CsiStreamService();
    const updates: string[] = [];
    service.subscribe((u) => updates.push(u.origin));
    await service.start({ enginePreference: 'js' });
    frameCallback!(makeFrame({ variance: 0.1 }));

    service.setConnectionStatus('disconnected');
    const snap = service.getSnapshot();
    expect(snap.offline).toBe(true);
    expect(snap.origin).toBe('local');
    expect(snap.result).not.toBeNull();
    expect(snap.result!.backend).toBe('js');
    service.stop();
  });

  it('keeps producing estimates from the last frame on the offline tick', async () => {
    const service = new CsiStreamService();
    let lastTimestamp = 0;
    service.subscribe((u) => {
      if (u.result) lastTimestamp = u.result.timestamp;
    });
    await service.start({ enginePreference: 'js' });
    frameCallback!(makeFrame({ variance: 0.1 }, undefined, 1234));
    service.setConnectionStatus('disconnected');

    const before = service.getSnapshot().result;
    expect(before).not.toBeNull();

    // Advance the offline tick — inference re-runs on the buffered frame.
    jest.advanceTimersByTime(1100);
    expect(lastTimestamp).toBe(1234);
    service.stop();
  });

  it('forceLocal makes origin local even while connected', async () => {
    const service = new CsiStreamService();
    await service.start({ enginePreference: 'js', forceLocal: true });
    service.setConnectionStatus('connected');
    frameCallback!(makeFrame({ variance: 0.1 }));
    const snap = service.getSnapshot();
    expect(snap.offline).toBe(true);
    expect(snap.origin).toBe('local');
    service.stop();
  });

  it('detects simulated frames as offline', async () => {
    const service = new CsiStreamService();
    await service.start({ enginePreference: 'js' });
    service.setConnectionStatus('connected');
    frameCallback!(makeFrame({ variance: 0.1 }, 'simulated'));
    const snap = service.getSnapshot();
    expect(snap.offline).toBe(true);
    service.stop();
  });

  it('setEnginePreference swaps the backend', async () => {
    const service = new CsiStreamService();
    await service.start({ enginePreference: 'js' });
    service.setConnectionStatus('disconnected');
    await service.setEnginePreference('wasm');
    frameCallback!(makeFrame({ variance: 0.1 }));
    const snap = service.getSnapshot();
    // wasm engine reports 'wasm' backend (delegating to JS internally).
    expect(snap.result!.backend).toBe('wasm');
    service.stop();
  });

  it('stop cleans up subscription and buffer', async () => {
    const service = new CsiStreamService();
    await service.start({ enginePreference: 'js' });
    frameCallback!(makeFrame({ variance: 0.1 }));
    service.stop();
    expect(service.bufferedCount).toBe(0);
    expect(frameCallback).toBeNull();
  });
});
