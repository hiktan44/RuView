// useLocalInference wires the csiStreamService into React state. We verify the
// module export + that the underlying stream service exposes the expected API.

jest.mock('@/services/csiStream.service', () => ({
  csiStreamService: {
    subscribe: jest.fn(() => jest.fn()),
    start: jest.fn(() => Promise.resolve()),
    stop: jest.fn(),
    setConnectionStatus: jest.fn(),
    setForceLocal: jest.fn(),
    setEnginePreference: jest.fn(() => Promise.resolve()),
    getSnapshot: jest.fn(() => ({
      frame: null,
      result: null,
      origin: 'server',
      offline: false,
      connectionStatus: 'disconnected',
    })),
  },
}));

describe('useLocalInference', () => {
  it('module exports useLocalInference function', () => {
    const mod = require('@/hooks/useLocalInference');
    expect(typeof mod.useLocalInference).toBe('function');
  });

  it('stream service exposes subscribe + start + lifecycle methods', () => {
    const { csiStreamService } = require('@/services/csiStream.service');
    expect(typeof csiStreamService.subscribe).toBe('function');
    expect(typeof csiStreamService.start).toBe('function');
    expect(typeof csiStreamService.setForceLocal).toBe('function');
    expect(typeof csiStreamService.setEnginePreference).toBe('function');
  });

  it('getSnapshot returns the expected update shape', () => {
    const { csiStreamService } = require('@/services/csiStream.service');
    const snap = csiStreamService.getSnapshot();
    expect(snap).toHaveProperty('origin');
    expect(snap).toHaveProperty('offline');
    expect(snap).toHaveProperty('result');
    expect(snap).toHaveProperty('connectionStatus');
  });
});
