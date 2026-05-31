import { Modal, Pressable, View, Text, StyleSheet } from 'react-native';
import { colors } from '@/theme/colors';
import { spacing } from '@/theme/spacing';
import type { Survivor } from '@/types/mat';
import { TRIAGE_COLOR, TRIAGE_META } from '@/services/triage.service';

type SurvivorDetailProps = {
  survivor: Survivor | null;
  onClose: () => void;
};

const fmt = (v: number) => (v > 0 ? String(v) : '--');

const formatIso = (iso: string): string => {
  try {
    return new Date(iso).toLocaleTimeString();
  } catch {
    return iso;
  }
};

export const SurvivorDetail = ({ survivor, onClose }: SurvivorDetailProps) => {
  if (!survivor) {
    return null;
  }
  const { bg, fg } = TRIAGE_COLOR[survivor.triage_status];
  const meta = TRIAGE_META[survivor.triage_status];

  return (
    <Modal visible transparent animationType="fade" onRequestClose={onClose}>
      <Pressable style={styles.backdrop} onPress={onClose}>
        <Pressable style={styles.card} onPress={() => undefined}>
          <View style={[styles.header, { backgroundColor: bg }]}>
            <Text style={[styles.title, { color: fg }]}>
              {survivor.zone_id || survivor.id} · {meta.label}
            </Text>
          </View>
          <View style={styles.body}>
            <Text style={styles.line}>ID: {survivor.id}</Text>
            <Text style={styles.line}>Breathing: {fmt(survivor.breathing_rate)} breaths/min</Text>
            <Text style={styles.line}>Heart rate: {fmt(survivor.heart_rate)} bpm</Text>
            <Text style={styles.line}>Confidence: {Math.round(survivor.confidence * 100)}%</Text>
            <Text style={styles.line}>
              Location: x {survivor.x.toFixed(2)}, y {survivor.y.toFixed(2)}, depth{' '}
              {survivor.depth.toFixed(2)} m
            </Text>
            <Text style={styles.line}>
              Deteriorating: {survivor.is_deteriorating ? 'yes' : 'no'}
            </Text>
            <Text style={styles.line}>First detected: {formatIso(survivor.first_detected)}</Text>
            <Text style={styles.line}>Last update: {formatIso(survivor.last_updated)}</Text>
          </View>
          <Pressable style={styles.close} onPress={onClose}>
            <Text style={styles.closeText}>Close</Text>
          </Pressable>
        </Pressable>
      </Pressable>
    </Modal>
  );
};

const styles = StyleSheet.create({
  backdrop: {
    flex: 1,
    backgroundColor: 'rgba(0,0,0,0.7)',
    justifyContent: 'center',
    padding: spacing.xl,
  },
  card: {
    backgroundColor: colors.surface,
    borderRadius: 12,
    overflow: 'hidden',
    borderWidth: 1,
    borderColor: colors.border,
  },
  header: { padding: spacing.md },
  title: { fontSize: 18, fontWeight: '700' },
  body: { padding: spacing.md },
  line: { fontSize: 14, color: colors.textPrimary, marginBottom: spacing.xs },
  close: {
    padding: spacing.md,
    alignItems: 'center',
    borderTopWidth: 1,
    borderTopColor: colors.border,
  },
  closeText: { color: colors.accent, fontWeight: '600' },
});
