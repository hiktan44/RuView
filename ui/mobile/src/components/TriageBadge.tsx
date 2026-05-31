import { View, Text, StyleSheet } from 'react-native';
import { spacing } from '@/theme/spacing';
import { TriageStatus } from '@/types/mat';
import { TRIAGE_COLOR, TRIAGE_META } from '@/services/triage.service';

type TriageBadgeProps = {
  status: TriageStatus;
  /** Show colour name (RED/YELLOW/...) instead of the protocol label. */
  short?: boolean;
};

/**
 * High-contrast START triage badge (sunlight-readable). Colour-coded
 * background with a readable foreground per category.
 */
export const TriageBadge = ({ status, short = false }: TriageBadgeProps) => {
  const { bg, fg } = TRIAGE_COLOR[status];
  const meta = TRIAGE_META[status];
  return (
    <View style={[styles.badge, { backgroundColor: bg }]}>
      <Text style={[styles.label, { color: fg }]}>{short ? meta.short : meta.label}</Text>
    </View>
  );
};

const styles = StyleSheet.create({
  badge: {
    paddingHorizontal: spacing.sm,
    paddingVertical: spacing.xs,
    borderRadius: 6,
    alignSelf: 'flex-start',
    minWidth: 88,
    alignItems: 'center',
  },
  label: {
    fontSize: 11,
    fontWeight: '700',
    letterSpacing: 0.5,
  },
});
