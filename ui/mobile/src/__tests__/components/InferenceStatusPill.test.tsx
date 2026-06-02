import React from 'react';
import { render, screen } from '@testing-library/react-native';
import { ThemeProvider } from '@/theme/ThemeContext';
import { InferenceStatusPill, formatAge } from '@/components/InferenceStatusPill';

const renderWithTheme = (ui: React.ReactElement) => render(<ThemeProvider>{ui}</ThemeProvider>);

describe('formatAge', () => {
  it('returns "no data" for null', () => {
    expect(formatAge(null)).toBe('veri yok');
  });

  it('returns "just now" for very recent timestamps', () => {
    const now = 10_000;
    expect(formatAge(now - 500, now)).toBe('az önce');
  });

  it('formats seconds ago', () => {
    const now = 100_000;
    expect(formatAge(now - 5_000, now)).toBe('5sn önce');
  });

  it('formats minutes ago', () => {
    const now = 1_000_000;
    expect(formatAge(now - 120_000, now)).toBe('2dk önce');
  });
});

describe('InferenceStatusPill', () => {
  it('renders ONLINE label when not offline', () => {
    renderWithTheme(<InferenceStatusPill offline={false} lastUpdated={Date.now()} />);
    expect(screen.getByText('ÇEVRİMİÇİ (sunucu çıkarımı)')).toBeTruthy();
  });

  it('renders OFFLINE label and backend when offline', () => {
    renderWithTheme(<InferenceStatusPill offline backend="js" lastUpdated={Date.now()} />);
    expect(screen.getByText('ÇEVRİMDIŞI (yerel çıkarım)')).toBeTruthy();
  });
});
