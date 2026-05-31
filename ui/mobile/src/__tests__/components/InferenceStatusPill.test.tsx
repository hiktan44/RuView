import React from 'react';
import { render, screen } from '@testing-library/react-native';
import { ThemeProvider } from '@/theme/ThemeContext';
import { InferenceStatusPill, formatAge } from '@/components/InferenceStatusPill';

const renderWithTheme = (ui: React.ReactElement) => render(<ThemeProvider>{ui}</ThemeProvider>);

describe('formatAge', () => {
  it('returns "no data" for null', () => {
    expect(formatAge(null)).toBe('no data');
  });

  it('returns "just now" for very recent timestamps', () => {
    const now = 10_000;
    expect(formatAge(now - 500, now)).toBe('just now');
  });

  it('formats seconds ago', () => {
    const now = 100_000;
    expect(formatAge(now - 5_000, now)).toBe('5s ago');
  });

  it('formats minutes ago', () => {
    const now = 1_000_000;
    expect(formatAge(now - 120_000, now)).toBe('2m ago');
  });
});

describe('InferenceStatusPill', () => {
  it('renders ONLINE label when not offline', () => {
    renderWithTheme(<InferenceStatusPill offline={false} lastUpdated={Date.now()} />);
    expect(screen.getByText('ONLINE (server inference)')).toBeTruthy();
  });

  it('renders OFFLINE label and backend when offline', () => {
    renderWithTheme(<InferenceStatusPill offline backend="js" lastUpdated={Date.now()} />);
    expect(screen.getByText('OFFLINE (local inference)')).toBeTruthy();
  });
});
