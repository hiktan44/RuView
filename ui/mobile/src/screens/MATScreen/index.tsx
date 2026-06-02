import { useCallback, useMemo, useState } from 'react';
import {
  View,
  Text,
  StyleSheet,
  FlatList,
  Pressable,
  TextInput,
  RefreshControl,
} from 'react-native';
import { ConnectionBanner } from '@/components/ConnectionBanner';
import { ThemedView } from '@/components/ThemedView';
import { colors } from '@/theme/colors';
import { spacing } from '@/theme/spacing';
import { usePoseStream } from '@/hooks/usePoseStream';
import { useTriageFeed } from '@/hooks/useTriageFeed';
import { useMatStore } from '@/stores/matStore';
import { type ConnectionStatus } from '@/types/sensing';
import { type Survivor } from '@/types/mat';
import { TRIAGE_PRIORITY } from '@/services/triage.service';
import { ZoneMap } from './ZoneMap';
import { SurvivorRow } from './SurvivorRow';
import { SurvivorDetail } from './SurvivorDetail';
import { TriageSummary } from './TriageSummary';

const resolveBannerState = (
  status: ConnectionStatus,
): 'connected' | 'simulated' | 'disconnected' => {
  if (status === 'connecting') {
    return 'disconnected';
  }
  return status;
};

/** Sort survivors by rescue priority (RED first), then shallower depth, then recency. */
const sortByPriority = (list: Survivor[]): Survivor[] =>
  [...list].sort((a, b) => {
    const p = TRIAGE_PRIORITY[a.triage_status] - TRIAGE_PRIORITY[b.triage_status];
    if (p !== 0) {
      return p;
    }
    const d = a.depth - b.depth;
    if (d !== 0) {
      return d;
    }
    return Date.parse(b.last_updated) - Date.parse(a.last_updated);
  });

export const MATScreen = () => {
  const { connectionStatus } = usePoseStream();
  const { connect, startDemo, muted, toggleMute } = useTriageFeed();

  const survivors = useMatStore((state) => state.survivors);
  const sorted = useMemo(() => sortByPriority(survivors), [survivors]);

  const [hostInput, setHostInput] = useState('');
  const [refreshing, setRefreshing] = useState(false);
  const [selected, setSelected] = useState<Survivor | null>(null);

  const onRefresh = useCallback(() => {
    setRefreshing(true);
    if (hostInput.trim()) {
      connect(hostInput);
    } else {
      startDemo();
    }
    setTimeout(() => setRefreshing(false), 600);
  }, [hostInput, connect, startDemo]);

  return (
    <ThemedView style={styles.container}>
      <ConnectionBanner status={resolveBannerState(connectionStatus)} />

      <FlatList
        data={sorted}
        keyExtractor={(s) => s.id}
        contentContainerStyle={styles.content}
        refreshControl={
          <RefreshControl
            refreshing={refreshing}
            onRefresh={onRefresh}
            tintColor={colors.textPrimary}
          />
        }
        ListHeaderComponent={
          <View>
            <View style={styles.titleRow}>
              <Text style={styles.title}>WiFi-MAT Triyaj</Text>
              <Pressable
                onPress={toggleMute}
                style={[styles.muteBtn, muted ? styles.muteOff : styles.muteOn]}
                accessibilityRole="button"
                accessibilityLabel={muted ? 'Alarmın sesini aç' : 'Alarmı sessize al'}
              >
                <Text style={styles.muteText}>{muted ? 'ALARM SESSİZ' : 'ALARM AÇIK'}</Text>
              </Pressable>
            </View>

            <TriageSummary survivors={survivors} />

            <View style={styles.card}>
              <Text style={styles.label}>Algılama sunucusu (sunucu:port)</Text>
              <View style={styles.connectRow}>
                <TextInput
                  style={styles.input}
                  value={hostInput}
                  onChangeText={setHostInput}
                  placeholder="192.168.4.1:8080"
                  placeholderTextColor={colors.textSecondary}
                  autoCapitalize="none"
                  autoCorrect={false}
                  accessibilityLabel="Algılama sunucusu adresi"
                />
                <Pressable
                  style={styles.connectBtn}
                  onPress={() => hostInput.trim() && connect(hostInput)}
                  accessibilityRole="button"
                >
                  <Text style={styles.connectBtnText}>Bağlan</Text>
                </Pressable>
              </View>
              <Pressable onPress={startDemo} accessibilityRole="button">
                <Text style={styles.demoLink}>Çevrimdışı demoyu başlat</Text>
              </Pressable>
            </View>

            <View style={styles.card}>
              <ZoneMap survivors={sorted} selectedId={selected?.id} />
            </View>

            <Text style={styles.sectionTitle}>Kazazedeler ({survivors.length})</Text>
          </View>
        }
        renderItem={({ item }) => (
          <SurvivorRow survivor={item} onPress={() => setSelected(item)} />
        )}
        ListEmptyComponent={
          <View style={styles.card}>
            <Text style={styles.empty}>
              Kazazede tespit edilmedi. Bir algılama düğümüne bağlanın veya demoyu başlatın.
            </Text>
          </View>
        }
      />

      <SurvivorDetail survivor={selected} onClose={() => setSelected(null)} />
    </ThemedView>
  );
};

export default MATScreen;

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.bg },
  content: { padding: spacing.md, paddingTop: 36, paddingBottom: spacing.xxl },
  titleRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    marginBottom: spacing.md,
  },
  title: { fontSize: 24, fontWeight: '700', color: colors.textPrimary },
  muteBtn: {
    paddingHorizontal: spacing.sm,
    paddingVertical: spacing.xs,
    borderRadius: 6,
  },
  muteOn: { backgroundColor: colors.success },
  muteOff: { backgroundColor: colors.muted },
  muteText: { color: '#FFFFFF', fontSize: 11, fontWeight: '700' },
  card: {
    backgroundColor: colors.surface,
    borderRadius: 12,
    borderWidth: 1,
    borderColor: colors.border,
    padding: spacing.md,
    marginBottom: spacing.md,
  },
  label: { fontSize: 12, color: colors.textSecondary, marginBottom: spacing.xs },
  connectRow: { flexDirection: 'row', gap: spacing.sm, alignItems: 'center' },
  input: {
    flex: 1,
    backgroundColor: colors.surfaceAlt,
    color: colors.textPrimary,
    borderRadius: 8,
    paddingHorizontal: spacing.sm,
    paddingVertical: spacing.sm,
  },
  connectBtn: {
    backgroundColor: colors.accent,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    borderRadius: 8,
  },
  connectBtnText: { color: '#06222A', fontWeight: '700' },
  demoLink: { color: colors.accent, fontSize: 13, marginTop: spacing.sm },
  sectionTitle: {
    fontSize: 16,
    fontWeight: '600',
    color: colors.textPrimary,
    marginBottom: spacing.sm,
  },
  empty: { fontSize: 14, color: colors.textSecondary },
});
