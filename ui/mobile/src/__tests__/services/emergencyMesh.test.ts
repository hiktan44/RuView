import {
  EMESH_GATEWAY,
  EMESH_PORT,
  rescueSsid,
  rescueWsUrl,
  rescueJoinSteps,
  isEmergencyMeshHost,
} from '@/services/emergencyMesh';

describe('emergencyMesh', () => {
  it('builds the rescue SSID for a node', () => {
    expect(rescueSsid(0)).toBe('RuView-Rescue-0');
    expect(rescueSsid(2)).toBe('RuView-Rescue-2');
  });

  it('builds the SoftAP gateway WebSocket URL', () => {
    expect(rescueWsUrl()).toBe(`ws://${EMESH_GATEWAY}:${EMESH_PORT}/ws/sensing`);
    expect(rescueWsUrl('192.168.4.1', 5005)).toBe('ws://192.168.4.1:5005/ws/sensing');
  });

  it('produces actionable join steps mentioning the SSID and gateway', () => {
    const steps = rescueJoinSteps(1);
    expect(steps.length).toBeGreaterThanOrEqual(4);
    expect(steps.some((s) => s.includes('RuView-Rescue-1'))).toBe(true);
    expect(steps.some((s) => s.includes(EMESH_GATEWAY))).toBe(true);
  });

  it('recognises emergency-mesh SoftAP hosts', () => {
    expect(isEmergencyMeshHost('192.168.4.1')).toBe(true);
    expect(isEmergencyMeshHost('192.168.4.2')).toBe(true);
    expect(isEmergencyMeshHost('ws://192.168.4.1:5005')).toBe(true);
    expect(isEmergencyMeshHost('192.168.1.20')).toBe(false);
    expect(isEmergencyMeshHost('seymata.com')).toBe(false);
  });
});
