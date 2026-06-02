import { Pressable, View, Text, StyleSheet } from 'react-native';
import { colors } from '@/theme/colors';
import { spacing } from '@/theme/spacing';
import type { Survivor } from '@/types/mat';
import { TRIAGE_COLOR } from '@/services/triage.service';
import { TriageBadge } from '@/components/TriageBadge';

type SurvivorRowProps = {
  survivor: Survivor;
  onPress: () => void;
};

const timeSince = (iso: string): string => {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) {
    return 'bilinmiyor';
  }
  const s = Math.max(0, Math.floor((Date.now() - then) / 1000));
  if (s < 60) {
    return `${s}sn önce`;
  }
  const m = Math.floor(s / 60);
  if (m < 60) {
    return `${m}dk ${s % 60}sn önce`;
  }
  return `${Math.floor(m / 60)}sa ${m % 60}dk önce`;
};

const fmtVital = (v: number): string => (v > 0 ? String(v) : '--');

export const SurvivorRow = ({ survivor, onPress }: SurvivorRowProps) => {
  const { bg } = TRIAGE_COLOR[survivor.triage_status];
  return (
    <Pressable onPress={onPress} accessibilityRole="button">
      <View style={[styles.row, { borderLeftColor: bg }]}>
        <View style={styles.top}>
          <TriageBadge status={survivor.triage_status} />
          <Text style={styles.id}>{survivor.zone_id || survivor.id}</Text>
          {survivor.is_deteriorating && <Text style={styles.deteriorating}>▼ KÖTÜLEŞİYOR</Text>}
        </View>
        <Text style={styles.vitals}>
          Solunum {fmtVital(survivor.breathing_rate)} /dk · Nabız {fmtVital(survivor.heart_rate)} bpm ·{' '}
          {Math.round(survivor.confidence * 100)}%
        </Text>
        <Text style={styles.meta}>
          x{survivor.x.toFixed(1)} y{survivor.y.toFixed(1)} · derinlik {survivor.depth.toFixed(1)}m ·{' '}
          {timeSince(survivor.last_updated)}
        </Text>
      </View>
    </Pressable>
  );
};

const styles = StyleSheet.create({
  row: {
    backgroundColor: colors.surface,
    borderRadius: 10,
    borderWidth: 1,
    borderColor: colors.border,
    borderLeftWidth: 6,
    padding: spacing.md,
    marginBottom: spacing.sm,
  },
  top: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: spacing.sm,
    marginBottom: spacing.xs,
    flexWrap: 'wrap',
  },
  id: {
    fontSize: 14,
    fontWeight: '600',
    color: colors.textPrimary,
  },
  deteriorating: {
    fontSize: 10,
    fontWeight: '700',
    color: colors.danger,
  },
  vitals: {
    fontSize: 13,
    color: colors.textPrimary,
    marginBottom: 2,
  },
  meta: {
    fontSize: 11,
    color: colors.textSecondary,
  },
});
