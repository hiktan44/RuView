import { StyleSheet, View } from 'react-native';
import { ThemedText } from './ThemedText';
import { colors } from '@/theme/colors';
import type { InferenceBackendKind } from '@/inference';

type InferenceStatusPillProps = {
  offline: boolean;
  /** Which backend produced the latest local result (shown when offline). */
  backend?: InferenceBackendKind;
  /** Epoch millis of the last result. */
  lastUpdated: number | null;
};

const formatAge = (lastUpdated: number | null, now = Date.now()): string => {
  if (lastUpdated == null) return 'veri yok';
  const ageMs = Math.max(0, now - lastUpdated);
  if (ageMs < 1500) return 'az önce';
  const seconds = Math.round(ageMs / 1000);
  if (seconds < 60) return `${seconds}sn önce`;
  const minutes = Math.round(seconds / 60);
  return `${minutes}dk önce`;
};

/**
 * Compact status indicator showing whether presence/vitals are coming from the
 * server (ONLINE) or being computed on-device (OFFLINE), plus a last-updated
 * timestamp. Used on LiveScreen and VitalsScreen.
 */
export const InferenceStatusPill = ({ offline, backend, lastUpdated }: InferenceStatusPillProps) => {
  const accent = offline ? colors.warn : colors.connected;
  const label = offline ? 'ÇEVRİMDIŞI (yerel çıkarım)' : 'ÇEVRİMİÇİ (sunucu çıkarımı)';
  const detail = offline && backend ? `${backend.toUpperCase()} · ${formatAge(lastUpdated)}` : formatAge(lastUpdated);

  return (
    <View
      style={[styles.pill, { borderColor: accent, backgroundColor: `${accent}1A` }]}
      accessibilityRole="text"
      accessibilityLabel={`${label}, güncellendi ${formatAge(lastUpdated)}`}
    >
      <View style={[styles.dot, { backgroundColor: accent }]} />
      <ThemedText preset="labelMd" style={[styles.label, { color: accent }]}>
        {label}
      </ThemedText>
      <ThemedText preset="bodySm" style={styles.detail} color="textSecondary">
        {detail}
      </ThemedText>
    </View>
  );
};

const styles = StyleSheet.create({
  pill: {
    flexDirection: 'row',
    alignItems: 'center',
    alignSelf: 'flex-start',
    gap: 8,
    borderWidth: 1,
    borderRadius: 999,
    paddingHorizontal: 12,
    paddingVertical: 6,
  },
  dot: {
    width: 8,
    height: 8,
    borderRadius: 4,
  },
  label: {
    letterSpacing: 0.5,
  },
  detail: {
    marginLeft: 2,
  },
});

export { formatAge };
