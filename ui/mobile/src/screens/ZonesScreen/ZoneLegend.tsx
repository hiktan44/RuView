import { View } from 'react-native';
import { ThemedText } from '@/components/ThemedText';
import { colors } from '@/theme/colors';
import { spacing } from '@/theme/spacing';
import { valueToColor } from '@/utils/colorMap';

type LegendStop = {
  label: string;
  color: string;
};

const LEGEND_STOPS: LegendStop[] = [
  { label: 'Sessiz', color: colorToRgba(0) },
  { label: 'Düşük', color: colorToRgba(0.25) },
  { label: 'Orta', color: colorToRgba(0.5) },
  { label: 'Yüksek', color: colorToRgba(0.75) },
  { label: 'Hareketli', color: colorToRgba(1) },
];

function colorToRgba(value: number): string {
  const [r, g, b] = valueToColor(value);
  return `rgba(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)}, 1)`;
}

export const ZoneLegend = () => {
  return (
    <View style={{ flexDirection: 'row', justifyContent: 'space-between', marginTop: spacing.md }}>
      {LEGEND_STOPS.map((stop) => (
        <View
          key={stop.label}
          style={{
            flexDirection: 'row',
            alignItems: 'center',
            gap: 6,
          }}
        >
          <View
            style={{
              width: 14,
              height: 14,
              borderRadius: 3,
              backgroundColor: stop.color,
              borderColor: colors.border,
              borderWidth: 1,
            }}
          />
          <ThemedText preset="bodySm" style={{ color: colors.textSecondary }}>
            {stop.label}
          </ThemedText>
        </View>
      ))}
    </View>
  );
};
