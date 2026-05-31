jest.mock('@react-native-async-storage/async-storage', () =>
  require('@react-native-async-storage/async-storage/jest/async-storage-mock')
);

jest.mock('react-native-wifi-reborn', () => ({
  loadWifiList: jest.fn(async () => []),
}));

// react-native-reanimated v4 eagerly initializes the native worklets module on
// import, which throws "Native part of Worklets doesn't seem to be initialized"
// under jest. Mock react-native-worklets with a no-op API surface so the
// reanimated mock can be imported without a native runtime.
jest.mock('react-native-worklets', () => {
  const RuntimeKind = { ReactNative: 1, UI: 2, Worker: 3 };
  const noop = () => undefined;
  const passthrough = <T>(value: T) => value;
  // A callable object whose every property access returns itself, so unknown
  // worklets exports tolerate any shape (e.g. cache.set(...), module.foo()).
  const anyStub: unknown = new Proxy(function () {}, {
    get: (_t, prop) => (prop === '__esModule' ? true : anyStub),
    apply: () => undefined,
  });
  const known: Record<string, unknown> = {
    __esModule: true,
    RuntimeKind,
    WorkletsModule: anyStub,
    getRuntimeKind: () => RuntimeKind.ReactNative,
    runOnUI: (fn: unknown) => fn,
    runOnUISync: (fn: unknown) =>
      typeof fn === 'function' ? (fn as () => unknown)() : undefined,
    runOnJS: (fn: unknown) => fn,
    scheduleOnUI: noop,
    scheduleOnRN: noop,
    callMicrotasks: noop,
    createSerializable: passthrough,
    createSerializableNull: () => null,
    makeShareable: passthrough,
    makeShareableCloneRecursive: passthrough,
    isWorkletFunction: () => false,
  };
  return new Proxy(known, {
    get(target, prop: string) {
      if (prop in target) return target[prop];
      return anyStub;
    },
  });
});

jest.mock('react-native-reanimated', () =>
  require('react-native-reanimated/mock')
);

// Optional native alarm modules — the alarm service loads them defensively at
// runtime; mock them so the audio/haptic channels are no-ops under jest.
jest.mock('expo-haptics', () => ({
  notificationAsync: jest.fn(async () => undefined),
  NotificationFeedbackType: { Error: 'error', Warning: 'warning' },
}));

jest.mock('expo-audio', () => ({
  setAudioModeAsync: jest.fn(async () => undefined),
  createAudioPlayer: jest.fn(() => ({ play: jest.fn(), remove: jest.fn() })),
}));

jest.mock('react-native-webview', () => {
  const React = require('react');
  const { View } = require('react-native');

  const MockWebView = (props: unknown) => React.createElement(View, props);

  return {
    __esModule: true,
    default: MockWebView,
    WebView: MockWebView,
  };
});
