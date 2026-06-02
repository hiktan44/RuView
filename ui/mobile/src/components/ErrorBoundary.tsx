import { Component, ErrorInfo, ReactNode } from 'react';
import { Button, StyleSheet, View } from 'react-native';
import { ThemedText } from './ThemedText';
import { ThemedView } from './ThemedView';

type ErrorBoundaryProps = {
  children: ReactNode;
};

type ErrorBoundaryState = {
  hasError: boolean;
  error?: Error;
};

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('ErrorBoundary caught an error', error, errorInfo);
  }

  handleRetry = () => {
    this.setState({ hasError: false, error: undefined });
  };

  render() {
    if (this.state.hasError) {
      return (
        <ThemedView style={styles.container}>
          <ThemedText preset="displayMd">Bir şeyler ters gitti</ThemedText>
          <ThemedText preset="bodySm" style={styles.message}>
            {this.state.error?.message ?? 'Beklenmeyen bir hata oluştu.'}
          </ThemedText>
          <View style={styles.buttonWrap}>
            <Button title="Yeniden Dene" onPress={this.handleRetry} />
          </View>
        </ThemedView>
      );
    }

    return this.props.children;
  }
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
    padding: 20,
    gap: 12,
  },
  message: {
    textAlign: 'center',
  },
  buttonWrap: {
    marginTop: 8,
  },
});
