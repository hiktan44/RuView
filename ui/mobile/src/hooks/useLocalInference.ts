import { useEffect, useState } from 'react';
import { csiStreamService, type SensingStreamUpdate } from '@/services/csiStream.service';
import { usePoseStore } from '@/stores/poseStore';
import { useSettingsStore } from '@/stores/settingsStore';

export interface UseLocalInferenceResult extends SensingStreamUpdate {
  /** Human-readable mode label for the UI status pill. */
  modeLabel: 'ONLINE (server inference)' | 'OFFLINE (local inference)';
  /** Epoch millis of the most recent result, or null. */
  lastUpdated: number | null;
}

const INITIAL: SensingStreamUpdate = {
  frame: null,
  result: null,
  origin: 'server',
  offline: false,
  connectionStatus: 'disconnected',
};

/**
 * Subscribe a screen to the CSI stream + local inference pipeline.
 *
 * Keeps presence/vital estimates flowing whether the sensing-server is
 * connected (origin `server`) or gone (origin `local`, computed on-device).
 * Reacts to the persisted force-local toggle and engine preference.
 */
export function useLocalInference(): UseLocalInferenceResult {
  const forceLocal = useSettingsStore((s) => s.forceLocalInference);
  const enginePreference = useSettingsStore((s) => s.inferenceEngine);
  const connectionStatus = usePoseStore((s) => s.connectionStatus);

  const [update, setUpdate] = useState<SensingStreamUpdate>(INITIAL);

  // Start/stop the stream service for the lifetime of any subscribing screen.
  useEffect(() => {
    const unsubscribe = csiStreamService.subscribe(setUpdate);
    void csiStreamService.start({ enginePreference, forceLocal });
    setUpdate(csiStreamService.getSnapshot());
    return () => {
      unsubscribe();
    };
    // start() is idempotent; engine/force changes are handled by effects below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Mirror the poseStore connection status into the stream service so origin
  // (server vs local) and the offline timer stay in sync with the WS client.
  useEffect(() => {
    csiStreamService.setConnectionStatus(connectionStatus);
  }, [connectionStatus]);

  useEffect(() => {
    csiStreamService.setForceLocal(forceLocal);
  }, [forceLocal]);

  useEffect(() => {
    void csiStreamService.setEnginePreference(enginePreference);
  }, [enginePreference]);

  return {
    ...update,
    modeLabel: update.offline ? 'OFFLINE (local inference)' : 'ONLINE (server inference)',
    lastUpdated: update.result?.timestamp ?? null,
  };
}
