import { useCallback, useEffect, useState } from 'react';
import { discoveryService } from '@/services/discovery.service';
import { alarmService } from '@/services/alarm.service';
import { onMatRedAlert } from '@/stores/matStore';

export interface UseTriageFeed {
  /** Connect to a sensing-server host (host:port or ws/http URL). */
  connect: (host: string) => void;
  /** Tear down the live connection / simulation. */
  disconnect: () => void;
  /** Start the offline demo stream immediately. */
  startDemo: () => void;
  muted: boolean;
  toggleMute: () => void;
}

/**
 * Wires the discovery service (live WS + simulation fallback) and the alarm
 * service (RED-detection -> haptic + audio) into a screen's lifecycle.
 */
export function useTriageFeed(): UseTriageFeed {
  const [muted, setMuted] = useState<boolean>(alarmService.isMuted());

  useEffect(() => {
    void alarmService.init().then(() => setMuted(alarmService.isMuted()));

    const unsubRed = onMatRedAlert(() => {
      void alarmService.fire();
    });

    return () => {
      unsubRed();
      discoveryService.disconnect();
    };
  }, []);

  const connect = useCallback((host: string) => {
    void discoveryService.connect(host);
  }, []);

  const disconnect = useCallback(() => {
    discoveryService.disconnect();
  }, []);

  const startDemo = useCallback(() => {
    discoveryService.startSimulation();
  }, []);

  const toggleMute = useCallback(() => {
    void alarmService.toggleMute().then((value) => setMuted(value));
  }, []);

  return { connect, disconnect, startDemo, muted, toggleMute };
}
