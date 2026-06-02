import React from 'react';
import { render, screen } from '@testing-library/react-native';
import { ConnectionBanner } from '@/components/ConnectionBanner';
import { ThemeProvider } from '@/theme/ThemeContext';

const renderWithTheme = (ui: React.ReactElement) =>
  render(<ThemeProvider>{ui}</ThemeProvider>);

describe('ConnectionBanner', () => {
  it('renders LIVE STREAM text when connected', () => {
    renderWithTheme(<ConnectionBanner status="connected" />);
    expect(screen.getByText('CANLI YAYIN')).toBeTruthy();
  });

  it('renders DISCONNECTED text when disconnected', () => {
    renderWithTheme(<ConnectionBanner status="disconnected" />);
    expect(screen.getByText('BAĞLANTI KESİLDİ')).toBeTruthy();
  });

  it('renders SIMULATED DATA text when simulated', () => {
    renderWithTheme(<ConnectionBanner status="simulated" />);
    expect(screen.getByText('SİMÜLE VERİ')).toBeTruthy();
  });

  it('renders without crashing for each status', () => {
    const statuses: Array<'connected' | 'simulated' | 'disconnected'> = [
      'connected',
      'simulated',
      'disconnected',
    ];
    for (const status of statuses) {
      const { unmount } = renderWithTheme(<ConnectionBanner status={status} />);
      unmount();
    }
  });
});
