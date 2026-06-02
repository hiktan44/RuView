import React from 'react';
import { View, Text, StyleSheet } from 'react-native';
import Svg, { Rect, Circle, Line, Text as SvgText } from 'react-native-svg';
import { colors } from '@/theme/colors';
import { spacing } from '@/theme/spacing';
import type { Survivor } from '@/types/mat';
import { TRIAGE_COLOR } from '@/services/triage.service';

type ZoneMapProps = {
  survivors: Survivor[];
  selectedId?: string | null;
  width?: number;
  height?: number;
};

/**
 * Top-down zone map plotting survivors by (x, y) with colour-coded dots.
 * Coordinates auto-scale to the viewport; deeper survivors get a wider ring.
 */
export const ZoneMap = ({ survivors, selectedId, width = 320, height = 200 }: ZoneMapProps) => {
  const pad = 16;
  const innerW = width - pad * 2;
  const innerH = height - pad * 2;

  const xs = survivors.map((s) => s.x);
  const ys = survivors.map((s) => s.y);
  const minX = xs.length ? Math.min(...xs) : 0;
  const maxX = xs.length ? Math.max(...xs) : 1;
  const minY = ys.length ? Math.min(...ys) : 0;
  const maxY = ys.length ? Math.max(...ys) : 1;
  const spanX = maxX - minX || 1;
  const spanY = maxY - minY || 1;
  const maxDepth = Math.max(1, ...survivors.map((s) => s.depth));

  const project = (x: number, y: number) => ({
    px: pad + ((x - minX) / spanX) * innerW,
    py: pad + ((y - minY) / spanY) * innerH,
  });

  return (
    <View>
      <Svg width={width} height={height}>
        <Rect
          x={0}
          y={0}
          width={width}
          height={height}
          rx={10}
          fill={colors.surfaceAlt}
          stroke={colors.border}
          strokeWidth={1}
        />
        {[0.25, 0.5, 0.75].map((f) => (
          <React.Fragment key={`grid-${f}`}>
            <Line
              x1={pad + innerW * f}
              y1={pad}
              x2={pad + innerW * f}
              y2={pad + innerH}
              stroke={colors.border}
              strokeWidth={0.5}
            />
            <Line
              x1={pad}
              y1={pad + innerH * f}
              x2={pad + innerW}
              y2={pad + innerH * f}
              stroke={colors.border}
              strokeWidth={0.5}
            />
          </React.Fragment>
        ))}

        {survivors.map((s) => {
          const { px, py } = project(s.x, s.y);
          const { bg } = TRIAGE_COLOR[s.triage_status];
          const depthRing = 4 + (s.depth / maxDepth) * 6;
          const isSel = selectedId === s.id;
          return (
            <React.Fragment key={s.id}>
              {isSel && (
                <Circle cx={px} cy={py} r={14} fill="none" stroke={colors.accent} strokeWidth={2} />
              )}
              <Circle cx={px} cy={py} r={9} fill={bg} stroke="#000000" strokeWidth={1} />
              <Circle
                cx={px}
                cy={py}
                r={9 + depthRing}
                fill="none"
                stroke={bg}
                strokeWidth={1}
                opacity={0.4}
              />
            </React.Fragment>
          );
        })}

        {survivors.length === 0 && (
          <SvgText x={width / 2} y={height / 2} fill={colors.textSecondary} fontSize={13} textAnchor="middle">
            Kazazede tespit edilmedi
          </SvgText>
        )}
      </Svg>
      <Text style={styles.caption}>Üstten bölge haritası · halka genişliği = derinlik</Text>
    </View>
  );
};

const styles = StyleSheet.create({
  caption: {
    fontSize: 11,
    color: colors.textSecondary,
    marginTop: spacing.xs,
    textAlign: 'center',
  },
});
