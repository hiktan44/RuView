// Ambient declarations for static asset + optional native module imports.
declare module '*.wav' {
  const asset: number;
  export default asset;
}

// Optional native modules — loaded lazily and guarded by try/catch in
// alarm.service.ts. Declared ambiently so `tsc` passes whether or not the
// packages are installed in node_modules.
declare module 'expo-haptics';
declare module 'expo-audio';
