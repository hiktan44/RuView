// Training Service for WiFi-DensePose UI
// Manages training lifecycle, progress streaming, and CSI recordings.

import { buildWsUrl } from '../config/api.config.js';
import { apiService } from './api.service.js';

export class TrainingService {
  constructor() {
    this.progressSocket = null;
    this.listeners = {};
    this.logger = this.createLogger();
  }

  createLogger() {
    return {
      debug: (...args) => console.debug('[TRAIN-DEBUG]', new Date().toISOString(), ...args),
      info: (...args) => console.info('[TRAIN-INFO]', new Date().toISOString(), ...args),
      warn: (...args) => console.warn('[TRAIN-WARN]', new Date().toISOString(), ...args),
      error: (...args) => console.error('[TRAIN-ERROR]', new Date().toISOString(), ...args)
    };
  }

  // --- Event emitter helpers ---

  on(event, callback) {
    if (!this.listeners[event]) {
      this.listeners[event] = [];
    }
    this.listeners[event].push(callback);
    return () => this.off(event, callback);
  }

  off(event, callback) {
    if (!this.listeners[event]) return;
    this.listeners[event] = this.listeners[event].filter(cb => cb !== callback);
  }

  emit(event, data) {
    if (!this.listeners[event]) return;
    this.listeners[event].forEach(cb => {
      try { cb(data); } catch (err) { this.logger.error('Listener error', { event, err }); }
    });
  }

  // --- Training API methods ---

  async startTraining(config) {
    try {
      this.logger.info('Starting training', { config });
      const data = await apiService.post('/api/v1/train/start', config);
      this.emit('training-started', data);
      return data;
    } catch (error) {
      this.logger.error('Failed to start training', { error: error.message });
      throw error;
    }
  }

  async stopTraining() {
    try {
      this.logger.info('Stopping training');
      const data = await apiService.post('/api/v1/train/stop', {});
      this.emit('training-stopped', data);
      return data;
    } catch (error) {
      this.logger.error('Failed to stop training', { error: error.message });
      throw error;
    }
  }

  async getTrainingStatus() {
    try {
      const data = await apiService.get('/api/v1/train/status');
      return data;
    } catch (error) {
      this.logger.error('Failed to get training status', { error: error.message });
      throw error;
    }
  }

  async startPretraining(config) {
    try {
      this.logger.info('Starting pretraining', { config });
      const data = await apiService.post('/api/v1/train/pretrain', config);
      this.emit('training-started', data);
      return data;
    } catch (error) {
      this.logger.error('Failed to start pretraining', { error: error.message });
      throw error;
    }
  }

  async startLoraTraining(config) {
    try {
      this.logger.info('Starting LoRA training', { config });
      const data = await apiService.post('/api/v1/train/lora', config);
      this.emit('training-started', data);
      return data;
    } catch (error) {
      this.logger.error('Failed to start LoRA training', { error: error.message });
      throw error;
    }
  }

  // --- Recording API methods ---

  async listRecordings() {
    try {
      const data = await apiService.get('/api/v1/recording/list');
      return data?.recordings ?? [];
    } catch (error) {
      this.logger.error('Failed to list recordings', { error: error.message });
      throw error;
    }
  }

  async startRecording(config) {
    try {
      this.logger.info('Starting recording', { config });
      const data = await apiService.post('/api/v1/recording/start', config);
      this.emit('recording-started', data);
      return data;
    } catch (error) {
      this.logger.error('Failed to start recording', { error: error.message });
      throw error;
    }
  }

  async stopRecording() {
    try {
      this.logger.info('Stopping recording');
      const data = await apiService.post('/api/v1/recording/stop', {});
      this.emit('recording-stopped', data);
      return data;
    } catch (error) {
      this.logger.error('Failed to stop recording', { error: error.message });
      throw error;
    }
  }

  async deleteRecording(id) {
    try {
      this.logger.info('Deleting recording', { id });
      const data = await apiService.delete(
        `/api/v1/recording/${encodeURIComponent(id)}`
      );
      return data;
    } catch (error) {
      this.logger.error('Failed to delete recording', { id, error: error.message });
      throw error;
    }
  }

  // --- Training progress (status polling) ---
  //
  // The server exposes training status via GET /api/v1/train/status, not a
  // dedicated /ws/train/progress WebSocket (that endpoint doesn't exist, so the
  // old WS attempt always errored). We poll the status endpoint and emit
  // `progress` events from it, which is what the UI consumes.

  connectProgressStream() {
    if (this._progressTimer) {
      this.logger.warn('Progress polling already active');
      return this._progressTimer;
    }
    this.logger.info('Connecting progress stream (status polling)');
    this.emit('progress-connected', {});

    const poll = async () => {
      try {
        const status = await this.getTrainingStatus();
        this.emit('progress', status);
        // Stop polling once training is no longer running.
        if (status && status.status && status.status !== 'running') {
          this.disconnectProgressStream();
        }
      } catch (err) {
        // Status endpoint hiccup — keep the panel alive, don't spam errors.
        this.logger.warn('Progress poll failed', { error: err.message });
      }
    };

    // Poll immediately, then every 2s.
    poll();
    this._progressTimer = setInterval(poll, 2000);
    return this._progressTimer;
  }

  disconnectProgressStream() {
    if (this._progressTimer) {
      clearInterval(this._progressTimer);
      this._progressTimer = null;
      this.emit('progress-disconnected', {});
    }
    // Legacy WS cleanup (in case an old socket is still around).
    if (this.progressSocket) {
      this.progressSocket.close();
      this.progressSocket = null;
    }
  }

  dispose() {
    this.disconnectProgressStream();
    this.listeners = {};
    this.logger.info('TrainingService disposed');
  }
}

// Create singleton instance
export const trainingService = new TrainingService();
