import { Pressable, StyleSheet, Switch, View } from 'react-native';
import { ThemedText } from '@/components/ThemedText';
import { colors } from '@/theme/colors';
import { spacing } from '@/theme/spacing';
import { isWasmSupported } from '@/inference';
import type { InferenceEnginePreference } from '@/stores/settingsStore';

type InferenceSettingsProps = {
  forceLocal: boolean;
  engine: InferenceEnginePreference;
  onForceLocalChange: (value: boolean) => void;
  onEngineChange: (engine: InferenceEnginePreference) => void;
};

const ENGINE_OPTIONS: { value: InferenceEnginePreference; label: string }[] = [
  { value: 'auto', label: 'Auto' },
  { value: 'js', label: 'JS' },
  { value: 'wasm', label: 'WASM' },
];

/**
 * Settings controls for on-device inference: a force-local toggle and an engine
 * picker. Selections are persisted by the caller via the settings store.
 */
export const InferenceSettings = ({
  forceLocal,
  engine,
  onForceLocalChange,
  onEngineChange,
}: InferenceSettingsProps) => {
  const wasmAvailable = isWasmSupported();

  return (
    <View>
      <View style={styles.row}>
        <View style={styles.rowText}>
          <ThemedText preset="bodyMd">Force local inference</ThemedText>
          <ThemedText preset="bodySm" style={styles.help}>
            Always derive presence and vitals on-device, even when the server is connected.
          </ThemedText>
        </View>
        <Switch
          value={forceLocal}
          onValueChange={onForceLocalChange}
          trackColor={{ true: colors.accent, false: colors.surfaceAlt }}
          thumbColor={colors.textPrimary}
        />
      </View>

      <ThemedText preset="bodyMd" style={{ marginTop: spacing.md }}>
        Inference engine
      </ThemedText>
      <View style={styles.options}>
        {ENGINE_OPTIONS.map((option) => {
          const isActive = option.value === engine;
          const disabled = option.value === 'wasm' && !wasmAvailable;
          return (
            <Pressable
              key={option.value}
              disabled={disabled}
              onPress={() => onEngineChange(option.value)}
              style={[
                styles.option,
                {
                  borderColor: isActive ? colors.accent : colors.border,
                  backgroundColor: isActive ? `${colors.accent}20` : colors.surface,
                  opacity: disabled ? 0.4 : 1,
                },
              ]}
            >
              <ThemedText
                preset="bodySm"
                style={{ color: isActive ? colors.accent : colors.textSecondary, paddingVertical: 8 }}
              >
                {option.label}
              </ThemedText>
            </Pressable>
          );
        })}
      </View>
      <ThemedText preset="bodySm" style={styles.help}>
        {wasmAvailable
          ? 'WASM runtime detected. A real model can be dropped in; until then WASM falls back to the JS engine.'
          : 'WASM unavailable in this runtime — the JS engine is used (fully functional offline).'}
      </ThemedText>
    </View>
  );
};

const styles = StyleSheet.create({
  row: { flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center' },
  rowText: { flex: 1, paddingRight: spacing.sm },
  help: { color: colors.textSecondary, marginTop: spacing.xs },
  options: { flexDirection: 'row', gap: spacing.sm, marginTop: spacing.sm },
  option: { flex: 1, borderWidth: 1, borderRadius: 8, alignItems: 'center' },
});
