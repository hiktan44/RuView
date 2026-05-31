import AsyncStorage from '@react-native-async-storage/async-storage';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

export type Theme = 'light' | 'dark' | 'system';

/** Which local inference backend the user prefers ('auto' = pick best available). */
export type InferenceEnginePreference = 'auto' | 'js' | 'wasm';

export interface SettingsState {
  serverUrl: string;
  rssiScanEnabled: boolean;
  theme: Theme;
  alertSoundEnabled: boolean;
  /** When true, always run on-device inference even if the server is connected. */
  forceLocalInference: boolean;
  /** Preferred local inference backend. */
  inferenceEngine: InferenceEnginePreference;
  setServerUrl: (url: string) => void;
  setRssiScanEnabled: (value: boolean) => void;
  setTheme: (theme: Theme) => void;
  setAlertSoundEnabled: (value: boolean) => void;
  setForceLocalInference: (value: boolean) => void;
  setInferenceEngine: (engine: InferenceEnginePreference) => void;
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      serverUrl: 'http://localhost:3000',
      rssiScanEnabled: false,
      theme: 'system',
      alertSoundEnabled: true,
      forceLocalInference: false,
      inferenceEngine: 'auto',

      setServerUrl: (url) => {
        set({ serverUrl: url });
      },

      setRssiScanEnabled: (value) => {
        set({ rssiScanEnabled: value });
      },

      setTheme: (theme) => {
        set({ theme });
      },

      setAlertSoundEnabled: (value) => {
        set({ alertSoundEnabled: value });
      },

      setForceLocalInference: (value) => {
        set({ forceLocalInference: value });
      },

      setInferenceEngine: (engine) => {
        set({ inferenceEngine: engine });
      },
    }),
    {
      name: 'wifi-densepose-settings',
      storage: createJSONStorage(() => AsyncStorage),
    },
  ),
);
