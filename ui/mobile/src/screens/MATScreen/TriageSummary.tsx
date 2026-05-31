import { View, Text, StyleSheet } from 'react-native';
import { spacing } from '@/theme/spacing';
import { TriageStatus, type Survivor } from '@/types/mat';
import { TRIAGE_COLOR, TRIAGE_META } from '@/services/triage.service';

type TriageSummaryProps = {
  survivors: Survivor[];
};

const ORDER: TriageStatus[] = [
  TriageStatus.Immediate,
  TriageStatus.Delayed,
  TriageStatus.Minor,
  TriageStatus.Deceased,
];

/** Summary header: high-contrast count per START colour (RED first). */
export const TriageSummary = ({ survivors }: TriageSummaryProps) => {
  const counts = survivors.reduce<Record<number, number>>((acc, s) => {
    acc[s.triage_status] = (acc[s.triage_status] ?? 0) + 1;
    return acc;
  }, {});

  return (
    <View style={styles.row}>
      {ORDER.map((status) => {
        const { bg, fg } = TRIAGE_COLOR[status];
        return (
          <View key={status} style={[styles.cell, { backgroundColor: bg }]}>
            <Text style={[styles.count, { color: fg }]}>{counts[status] ?? 0}</Text>
            <Text style={[styles.label, { color: fg }]}>{TRIAGE_META[status].label}</Text>
          </View>
        );
      })}
    </View>
  );
};

const styles = StyleSheet.create({
  row: {
    flexDirection: 'row',
    gap: spacing.xs,
    marginBottom: spacing.md,
  },
  cell: {
    flex: 1,
    borderRadius: 8,
    paddingVertical: spacing.sm,
    alignItems: 'center',
  },
  count: { fontSize: 22, fontWeight: '700' },
  label: { fontSize: 9, fontWeight: '700', letterSpacing: 0.5 },
});
