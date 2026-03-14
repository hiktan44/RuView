// Dashboard Tab Component

import { healthService } from '../services/health.service.js';
import { poseService } from '../services/pose.service.js';
import { sensingService } from '../services/sensing.service.js';

export class DashboardTab {
  constructor(containerElement) {
    this.container = containerElement;
    this.statsElements = {};
    this.healthSubscription = null;
    this.statsInterval = null;
    
    // Yeni özellikler
    this.heatmapData = [];
    this.heatmapCtx = null;
    this.vitalData = { breathing: [], heart: [] };
    this.stickmanCtx = null;
    this.currentPose = null;
    this.theme = 'light';
  }

  // Initialize component
  async init() {
    this.cacheElements();
    await this.loadInitialData();
    this.startMonitoring();
    this.initHeatmap();
    this.initVitalSigns();
    this.initStickman();
    this.initThemeToggle();
    this.initExportModal();
  }

  // Cache DOM elements
  cacheElements() {
    // System stats
    const statsContainer = this.container.querySelector('.system-stats');
    if (statsContainer) {
      this.statsElements = {
        bodyRegions: statsContainer.querySelector('[data-stat="body-regions"] .stat-value'),
        samplingRate: statsContainer.querySelector('[data-stat="sampling-rate"] .stat-value'),
        accuracy: statsContainer.querySelector('[data-stat="accuracy"] .stat-value'),
        hardwareCost: statsContainer.querySelector('[data-stat="hardware-cost"] .stat-value')
      };
    }

    // Status indicators
    this.statusElements = {
      apiStatus: this.container.querySelector('.api-status'),
      streamStatus: this.container.querySelector('.stream-status'),
      hardwareStatus: this.container.querySelector('.hardware-status')
    };
  }

  // Load initial data
  async loadInitialData() {
    try {
      // Get API info
      const info = await healthService.getApiInfo();
      this.updateApiInfo(info);

      // Get current stats
      const stats = await poseService.getStats(1);
      this.updateStats(stats);

    } catch (error) {
      // DensePose API may not be running (sensing-only mode) — fail silently
      console.log('Dashboard: DensePose API not available (sensing-only mode)');
    }
  }

  // Start monitoring
  startMonitoring() {
    // Subscribe to health updates
    this.healthSubscription = healthService.subscribeToHealth(health => {
      this.updateHealthStatus(health);
    });

    // Subscribe to sensing service state changes for data source indicator
    this._sensingUnsub = sensingService.onStateChange(() => {
      this.updateDataSourceIndicator();
    });
    // Also update on data — catches source changes mid-stream
    this._sensingDataUnsub = sensingService.onData(() => {
      this.updateDataSourceIndicator();
    });
    // Initial update
    this.updateDataSourceIndicator();

    // Start periodic stats updates
    this.statsInterval = setInterval(() => {
      this.updateLiveStats();
    }, 5000);

    // Start health monitoring
    healthService.startHealthMonitoring(30000);
  }

  // Update the data source indicator on the dashboard
  updateDataSourceIndicator() {
    const el = this.container.querySelector('#dashboard-datasource');
    if (!el) return;
    const ds = sensingService.dataSource;
    const statusText = el.querySelector('.status-text');
    const statusMsg  = el.querySelector('.status-message');
    const config = {
      'live':              { text: 'ESP32',     status: 'healthy', msg: 'Real hardware connected' },
      'server-simulated':  { text: 'SIMULATED', status: 'warning', msg: 'Server running without hardware' },
      'reconnecting':      { text: 'RECONNECTING', status: 'degraded', msg: 'Attempting to connect...' },
      'simulated':         { text: 'OFFLINE',   status: 'unhealthy', msg: 'Server unreachable, local fallback' },
    };
    const cfg = config[ds] || config['reconnecting'];
    el.className = `component-status status-${cfg.status}`;
    if (statusText) statusText.textContent = cfg.text;
    if (statusMsg)  statusMsg.textContent = cfg.msg;
  }

  // Update API info display
  updateApiInfo(info) {
    // Update version
    const versionElement = this.container.querySelector('.api-version');
    if (versionElement && info.version) {
      versionElement.textContent = `v${info.version}`;
    }

    // Update environment
    const envElement = this.container.querySelector('.api-environment');
    if (envElement && info.environment) {
      envElement.textContent = info.environment;
      envElement.className = `api-environment env-${info.environment}`;
    }

    // Update features status
    if (info.features) {
      this.updateFeatures(info.features);
    }
  }

  // Update features display
  updateFeatures(features) {
    const featuresContainer = this.container.querySelector('.features-status');
    if (!featuresContainer) return;

    featuresContainer.innerHTML = '';
    
    Object.entries(features).forEach(([feature, enabled]) => {
      const featureElement = document.createElement('div');
      featureElement.className = `feature-item ${enabled ? 'enabled' : 'disabled'}`;
      
      // Use textContent instead of innerHTML to prevent XSS
      const featureNameSpan = document.createElement('span');
      featureNameSpan.className = 'feature-name';
      featureNameSpan.textContent = this.formatFeatureName(feature);
      
      const featureStatusSpan = document.createElement('span');
      featureStatusSpan.className = 'feature-status';
      featureStatusSpan.textContent = enabled ? '✓' : '✗';
      
      featureElement.appendChild(featureNameSpan);
      featureElement.appendChild(featureStatusSpan);
      featuresContainer.appendChild(featureElement);
    });
  }

  // Update health status
  updateHealthStatus(health) {
    if (!health) return;

    // Update overall status
    const overallStatus = this.container.querySelector('.overall-health');
    if (overallStatus) {
      overallStatus.className = `overall-health status-${health.status}`;
      overallStatus.textContent = health.status.toUpperCase();
    }

    // Update component statuses
    if (health.components) {
      Object.entries(health.components).forEach(([component, status]) => {
        this.updateComponentStatus(component, status);
      });
    }

    // Update metrics
    if (health.metrics) {
      this.updateSystemMetrics(health.metrics);
    }
  }

  // Update component status
  updateComponentStatus(component, status) {
    // Map backend component names to UI component names
    const componentMap = {
      'pose': 'inference',
      'stream': 'streaming',
      'hardware': 'hardware'
    };
    
    const uiComponent = componentMap[component] || component;
    const element = this.container.querySelector(`[data-component="${uiComponent}"]`);
    
    if (element) {
      element.className = `component-status status-${status.status}`;
      const statusText = element.querySelector('.status-text');
      const statusMessage = element.querySelector('.status-message');
      
      if (statusText) {
        statusText.textContent = status.status.toUpperCase();
      }
      
      if (statusMessage && status.message) {
        statusMessage.textContent = status.message;
      }
    }
    
    // Also update API status based on overall health
    if (component === 'hardware') {
      const apiElement = this.container.querySelector(`[data-component="api"]`);
      if (apiElement) {
        apiElement.className = `component-status status-healthy`;
        const apiStatusText = apiElement.querySelector('.status-text');
        const apiStatusMessage = apiElement.querySelector('.status-message');
        
        if (apiStatusText) {
          apiStatusText.textContent = 'HEALTHY';
        }
        
        if (apiStatusMessage) {
          apiStatusMessage.textContent = 'API server is running normally';
        }
      }
    }
  }

  // Update system metrics
  updateSystemMetrics(metrics) {
    // Handle both flat and nested metric structures
    // Backend returns system_metrics.cpu.percent, mock returns metrics.cpu.percent
    const systemMetrics = metrics.system_metrics || metrics;
    const cpuPercent = systemMetrics.cpu?.percent || systemMetrics.cpu_percent;
    const memoryPercent = systemMetrics.memory?.percent || systemMetrics.memory_percent;
    const diskPercent = systemMetrics.disk?.percent || systemMetrics.disk_percent;

    // CPU usage
    const cpuElement = this.container.querySelector('.cpu-usage');
    if (cpuElement && cpuPercent !== undefined) {
      cpuElement.textContent = `${cpuPercent.toFixed(1)}%`;
      this.updateProgressBar('cpu', cpuPercent);
    }

    // Memory usage
    const memoryElement = this.container.querySelector('.memory-usage');
    if (memoryElement && memoryPercent !== undefined) {
      memoryElement.textContent = `${memoryPercent.toFixed(1)}%`;
      this.updateProgressBar('memory', memoryPercent);
    }

    // Disk usage
    const diskElement = this.container.querySelector('.disk-usage');
    if (diskElement && diskPercent !== undefined) {
      diskElement.textContent = `${diskPercent.toFixed(1)}%`;
      this.updateProgressBar('disk', diskPercent);
    }
  }

  // Update progress bar
  updateProgressBar(type, percent) {
    const progressBar = this.container.querySelector(`.progress-bar[data-type="${type}"]`);
    if (progressBar) {
      const fill = progressBar.querySelector('.progress-fill');
      if (fill) {
        fill.style.width = `${percent}%`;
        fill.className = `progress-fill ${this.getProgressClass(percent)}`;
      }
    }
  }

  // Get progress class based on percentage
  getProgressClass(percent) {
    if (percent >= 90) return 'critical';
    if (percent >= 75) return 'warning';
    return 'normal';
  }

  // Update live statistics
  async updateLiveStats() {
    try {
      // Get current pose data
      const currentPose = await poseService.getCurrentPose();
      this.updatePoseStats(currentPose);

      // Get zones summary
      const zonesSummary = await poseService.getZonesSummary();
      this.updateZonesDisplay(zonesSummary);

    } catch (error) {
      console.error('Failed to update live stats:', error);
    }
  }

  // Update pose statistics
  updatePoseStats(poseData) {
    if (!poseData) return;

    // Update person count
    const personCount = this.container.querySelector('.person-count');
    if (personCount) {
      const count = poseData.persons ? poseData.persons.length : (poseData.total_persons || 0);
      personCount.textContent = count;
    }

    // Update average confidence
    const avgConfidence = this.container.querySelector('.avg-confidence');
    if (avgConfidence && poseData.persons && poseData.persons.length > 0) {
      const confidences = poseData.persons.map(p => p.confidence);
      const avg = confidences.length > 0
        ? (confidences.reduce((a, b) => a + b, 0) / confidences.length * 100).toFixed(1)
        : 0;
      avgConfidence.textContent = `${avg}%`;
    } else if (avgConfidence) {
      avgConfidence.textContent = '0%';
    }

    // Update total detections from stats if available
    const detectionCount = this.container.querySelector('.detection-count');
    if (detectionCount && poseData.total_detections !== undefined) {
      detectionCount.textContent = this.formatNumber(poseData.total_detections);
    }
  }

  // Update zones display
  updateZonesDisplay(zonesSummary) {
    const zonesContainer = this.container.querySelector('.zones-summary');
    if (!zonesContainer) return;

    zonesContainer.innerHTML = '';
    
    // Handle different zone summary formats
    let zones = {};
    if (zonesSummary && zonesSummary.zones) {
      zones = zonesSummary.zones;
    } else if (zonesSummary && typeof zonesSummary === 'object') {
      zones = zonesSummary;
    }
    
    // If no zones data, show default zones
    if (Object.keys(zones).length === 0) {
      ['zone_1', 'zone_2', 'zone_3', 'zone_4'].forEach(zoneId => {
        const zoneElement = document.createElement('div');
        zoneElement.className = 'zone-item';
        
        // Use textContent instead of innerHTML to prevent XSS
        const zoneNameSpan = document.createElement('span');
        zoneNameSpan.className = 'zone-name';
        zoneNameSpan.textContent = zoneId;
        
        const zoneCountSpan = document.createElement('span');
        zoneCountSpan.className = 'zone-count';
        zoneCountSpan.textContent = 'undefined';
        
        zoneElement.appendChild(zoneNameSpan);
        zoneElement.appendChild(zoneCountSpan);
        zonesContainer.appendChild(zoneElement);
      });
      return;
    }
    
    Object.entries(zones).forEach(([zoneId, data]) => {
      const zoneElement = document.createElement('div');
      zoneElement.className = 'zone-item';
      const count = typeof data === 'object' ? (data.person_count || data.count || 0) : data;
      
      // Use textContent instead of innerHTML to prevent XSS
      const zoneNameSpan = document.createElement('span');
      zoneNameSpan.className = 'zone-name';
      zoneNameSpan.textContent = zoneId;
      
      const zoneCountSpan = document.createElement('span');
      zoneCountSpan.className = 'zone-count';
      zoneCountSpan.textContent = String(count);
      
      zoneElement.appendChild(zoneNameSpan);
      zoneElement.appendChild(zoneCountSpan);
      zonesContainer.appendChild(zoneElement);
    });
  }

  // Update statistics
  updateStats(stats) {
    if (!stats) return;

    // Update detection count
    const detectionCount = this.container.querySelector('.detection-count');
    if (detectionCount && stats.total_detections !== undefined) {
      detectionCount.textContent = this.formatNumber(stats.total_detections);
    }

    // Update accuracy if available
    if (this.statsElements.accuracy && stats.average_confidence !== undefined) {
      this.statsElements.accuracy.textContent = `${(stats.average_confidence * 100).toFixed(1)}%`;
    }
  }

  // Format feature name
  formatFeatureName(name) {
    return name.replace(/_/g, ' ')
      .split(' ')
      .map(word => word.charAt(0).toUpperCase() + word.slice(1))
      .join(' ');
  }

  // Format large numbers
  formatNumber(num) {
    if (num >= 1000000) {
      return `${(num / 1000000).toFixed(1)}M`;
    }
    if (num >= 1000) {
      return `${(num / 1000).toFixed(1)}K`;
    }
    return num.toString();
  }

  // Show error message
  showError(message) {
    const errorContainer = this.container.querySelector('.error-container');
    if (errorContainer) {
      errorContainer.textContent = message;
      errorContainer.style.display = 'block';
      
      setTimeout(() => {
        errorContainer.style.display = 'none';
      }, 5000);
    }
  }

  // ========================================
  // ISI HARITASI (HEATMAP)
  // ========================================
  initHeatmap() {
    const canvas = document.getElementById('heatmapCanvas');
    if (!canvas) return;
    
    this.heatmapCtx = canvas.getContext('2d');
    this.heatmapData = Array(20).fill(null).map(() => Array(15).fill(0));
    
    // Sıfırlama butonu
    const resetBtn = document.getElementById('resetHeatmap');
    if (resetBtn) {
      resetBtn.addEventListener('click', () => {
        this.heatmapData = Array(20).fill(null).map(() => Array(15).fill(0));
        this.drawHeatmap();
      });
    }
    
    // Simülasyon için rastgele veri
    this.heatmapInterval = setInterval(() => {
      this.updateHeatmap();
    }, 500);
    
    this.drawHeatmap();
  }

  updateHeatmap() {
    // Rastgele hareket simülasyonu
    const x = Math.floor(Math.random() * 20);
    const y = Math.floor(Math.random() * 15);
    this.heatmapData[x][y] = Math.min(1, this.heatmapData[x][y] + 0.3);
    
    // Yayılma efekti
    for (let i = 0; i < 20; i++) {
      for (let j = 0; j < 15; j++) {
        this.heatmapData[i][j] *= 0.98;
      }
    }
    
    this.drawHeatmap();
  }

  drawHeatmap() {
    if (!this.heatmapCtx) return;
    
    const canvas = this.heatmapCtx.canvas;
    const cellWidth = canvas.width / 20;
    const cellHeight = canvas.height / 15;
    
    this.heatmapCtx.clearRect(0, 0, canvas.width, canvas.height);
    
    for (let i = 0; i < 20; i++) {
      for (let j = 0; j < 15; j++) {
        const value = this.heatmapData[i][j];
        const color = this.getHeatmapColor(value);
        this.heatmapCtx.fillStyle = color;
        this.heatmapCtx.fillRect(i * cellWidth, j * cellHeight, cellWidth, cellHeight);
      }
    }
  }

  getHeatmapColor(value) {
    // Yeşil -> Sarı -> Kırmızı gradyan
    const r = Math.floor(255 * Math.min(1, value * 2));
    const g = Math.floor(255 * Math.max(0, 1 - Math.abs(value - 0.5) * 2));
    const b = Math.floor(255 * Math.max(0, 1 - value * 2));
    return `rgba(${r}, ${g}, ${b}, ${0.3 + value * 0.7})`;
  }

  // ========================================
  // VITAL İŞARETLER
  // ========================================
  initVitalSigns() {
    const breathingCanvas = document.getElementById('breathingWave');
    const heartCanvas = document.getElementById('heartWave');
    
    if (breathingCanvas) {
      this.breathingCtx = breathingCanvas.getContext('2d');
    }
    if (heartCanvas) {
      this.heartCtx = heartCanvas.getContext('2d');
    }
    
    // Simülasyon
    this.vitalInterval = setInterval(() => {
      this.updateVitalSigns();
    }, 100);
  }

  updateVitalSigns() {
    const time = Date.now() / 1000;
    
    // Nefes (0.2 Hz = 12 BPM)
    const breathing = Math.sin(time * 0.2 * Math.PI * 2) * 0.5 + 0.5;
    this.vitalData.breathing.push(breathing);
    if (this.vitalData.breathing.length > 75) this.vitalData.breathing.shift();
    
    // Kalp (1.2 Hz = 72 BPM)
    const heart = Math.sin(time * 1.2 * Math.PI * 2) * 0.5 + 0.5;
    this.vitalData.heart.push(heart);
    if (this.vitalData.heart.length > 75) this.vitalData.heart.shift();
    
    // Değerleri güncelle
    const breathingRate = document.getElementById('breathingRate');
    const heartRate = document.getElementById('heartRate');
    
    if (breathingRate) {
      const bpm = Math.floor(12 + Math.random() * 6);
      breathingRate.textContent = `${bpm} BPM`;
    }
    if (heartRate) {
      const bpm = Math.floor(68 + Math.random() * 10);
      heartRate.textContent = `${bpm} BPM`;
    }
    
    // Çiz
    this.drawVitalWave('breathing');
    this.drawVitalWave('heart');
  }

  drawVitalWave(type) {
    const ctx = type === 'breathing' ? this.breathingCtx : this.heartCtx;
    const data = this.vitalData[type];
    
    if (!ctx || !data.length) return;
    
    const canvas = ctx.canvas;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    
    ctx.beginPath();
    ctx.strokeStyle = type === 'breathing' ? '#21a0c5' : '#e74c3c';
    ctx.lineWidth = 2;
    
    for (let i = 0; i < data.length; i++) {
      const x = (i / data.length) * canvas.width;
      const y = canvas.height - (data[i] * canvas.height * 0.8) - canvas.height * 0.1;
      
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    
    ctx.stroke();
  }

  // ========================================
  // ÇÖP ADAM (STICKMAN)
  // ========================================
  initStickman() {
    const canvas = document.getElementById('stickmanCanvas');
    if (!canvas) return;
    
    this.stickmanCtx = canvas.getContext('2d');
    
    // Poz verisi simülasyonu
    this.stickmanInterval = setInterval(() => {
      this.updateStickman();
    }, 100);
    
    this.drawStickman();
  }

  updateStickman() {
    const time = Date.now() / 1000;
    
    // Rastgele poz simülasyonu
    this.currentPose = {
      head: { x: 150, y: 50 },
      neck: { x: 150, y: 80 },
      leftShoulder: { x: 120, y: 90 },
      rightShoulder: { x: 180, y: 90 },
      leftElbow: { x: 100 + Math.sin(time) * 20, y: 130 + Math.cos(time) * 10 },
      rightElbow: { x: 200 + Math.sin(time + 1) * 20, y: 130 + Math.cos(time + 1) * 10 },
      leftWrist: { x: 90 + Math.sin(time * 1.5) * 30, y: 170 + Math.cos(time * 1.5) * 15 },
      rightWrist: { x: 210 + Math.sin(time * 1.5 + 1) * 30, y: 170 + Math.cos(time * 1.5 + 1) * 15 },
      spine: { x: 150, y: 150 },
      leftHip: { x: 130, y: 200 },
      rightHip: { x: 170, y: 200 },
      leftKnee: { x: 125 + Math.sin(time * 0.8) * 10, y: 280 + Math.abs(Math.sin(time * 0.8)) * 20 },
      rightKnee: { x: 175 + Math.sin(time * 0.8 + Math.PI) * 10, y: 280 + Math.abs(Math.sin(time * 0.8 + Math.PI)) * 20 },
      leftAnkle: { x: 120 + Math.sin(time * 0.8) * 15, y: 350 },
      rightAnkle: { x: 180 + Math.sin(time * 0.8 + Math.PI) * 15, y: 350 },
      confidence: 0.85 + Math.random() * 0.1
    };
    
    this.drawStickman();
    
    // Bilgileri güncelle
    const confEl = document.getElementById('poseConfidence');
    const personEl = document.getElementById('posePerson');
    
    if (confEl) confEl.textContent = `${Math.floor(this.currentPose.confidence * 100)}%`;
    if (personEl) personEl.textContent = '1';
  }

  drawStickman() {
    if (!this.stickmanCtx || !this.currentPose) {
      // Başlangıç durumu
      this.stickmanCtx = document.getElementById('stickmanCanvas')?.getContext('2d');
      if (!this.stickmanCtx) return;
      
      // Varsayılan poz
      this.currentPose = {
        head: { x: 150, y: 50 },
        neck: { x: 150, y: 80 },
        leftShoulder: { x: 120, y: 90 },
        rightShoulder: { x: 180, y: 90 },
        leftElbow: { x: 100, y: 130 },
        rightElbow: { x: 200, y: 130 },
        leftWrist: { x: 90, y: 170 },
        rightWrist: { x: 210, y: 170 },
        spine: { x: 150, y: 150 },
        leftHip: { x: 130, y: 200 },
        rightHip: { x: 170, y: 200 },
        leftKnee: { x: 125, y: 280 },
        rightKnee: { x: 175, y: 280 },
        leftAnkle: { x: 120, y: 350 },
        rightAnkle: { x: 180, y: 350 },
        confidence: 0.9
      };
    }
    
    const ctx = this.stickmanCtx;
    const canvas = ctx.canvas;
    
    // Temizle
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    
    const pose = this.currentPose;
    
    // Bağlantı çizgileri
    ctx.strokeStyle = '#21a0c5';
    ctx.lineWidth = 4;
    ctx.lineCap = 'round';
    
    // Baş-Boyun
    ctx.beginPath();
    ctx.moveTo(pose.head.x, pose.head.y);
    ctx.lineTo(pose.neck.x, pose.neck.y);
    ctx.stroke();
    
    // Gövde
    ctx.beginPath();
    ctx.moveTo(pose.neck.x, pose.neck.y);
    ctx.lineTo(pose.spine.x, pose.spine.y);
    ctx.stroke();
    
    // Omuzlar
    ctx.beginPath();
    ctx.moveTo(pose.leftShoulder.x, pose.leftShoulder.y);
    ctx.lineTo(pose.rightShoulder.x, pose.rightShoulder.y);
    ctx.stroke();
    
    // Sol Kol
    ctx.beginPath();
    ctx.moveTo(pose.leftShoulder.x, pose.leftShoulder.y);
    ctx.lineTo(pose.leftElbow.x, pose.leftElbow.y);
    ctx.lineTo(pose.leftWrist.x, pose.leftWrist.y);
    ctx.stroke();
    
    // Sağ Kol
    ctx.beginPath();
    ctx.moveTo(pose.rightShoulder.x, pose.rightShoulder.y);
    ctx.lineTo(pose.rightElbow.x, pose.rightElbow.y);
    ctx.lineTo(pose.rightWrist.x, pose.rightWrist.y);
    ctx.stroke();
    
    // Kalça
    ctx.beginPath();
    ctx.moveTo(pose.leftHip.x, pose.leftHip.y);
    ctx.lineTo(pose.rightHip.x, pose.rightHip.y);
    ctx.stroke();
    
    // Gövde-Kalça
    ctx.beginPath();
    ctx.moveTo(pose.spine.x, pose.spine.y);
    ctx.lineTo((pose.leftHip.x + pose.rightHip.x) / 2, (pose.leftHip.y + pose.rightHip.y) / 2);
    ctx.stroke();
    
    // Sol Bacak
    ctx.beginPath();
    ctx.moveTo(pose.leftHip.x, pose.leftHip.y);
    ctx.lineTo(pose.leftKnee.x, pose.leftKnee.y);
    ctx.lineTo(pose.leftAnkle.x, pose.leftAnkle.y);
    ctx.stroke();
    
    // Sağ Bacak
    ctx.beginPath();
    ctx.moveTo(pose.rightHip.x, pose.rightHip.y);
    ctx.lineTo(pose.rightKnee.x, pose.rightKnee.y);
    ctx.lineTo(pose.rightAnkle.x, pose.rightAnkle.y);
    ctx.stroke();
    
    // Eklem noktaları
    ctx.fillStyle = '#e74c3c';
    const joints = [
      pose.head, pose.neck, pose.leftShoulder, pose.rightShoulder,
      pose.leftElbow, pose.rightElbow, pose.leftWrist, pose.rightWrist,
      pose.spine, pose.leftHip, pose.rightHip,
      pose.leftKnee, pose.rightKnee, pose.leftAnkle, pose.rightAnkle
    ];
    
    joints.forEach(joint => {
      ctx.beginPath();
      ctx.arc(joint.x, joint.y, 6, 0, Math.PI * 2);
      ctx.fill();
    });
    
    // Baş (daha büyük)
    ctx.beginPath();
    ctx.arc(pose.head.x, pose.head.y - 15, 18, 0, Math.PI * 2);
    ctx.strokeStyle = '#21a0c5';
    ctx.lineWidth = 3;
    ctx.stroke();
  }

  // ========================================
  // TEMA DEĞİŞTİRME
  // ========================================
  initThemeToggle() {
    const themeBtn = document.getElementById('themeToggle');
    if (!themeBtn) return;
    
    // Kayıtlı temayı yükle
    const savedTheme = localStorage.getItem('ruview-theme') || 'light';
    this.setTheme(savedTheme);
    
    themeBtn.addEventListener('click', () => {
      this.toggleTheme();
    });
  }

  toggleTheme() {
    this.theme = this.theme === 'light' ? 'dark' : 'light';
    this.setTheme(this.theme);
  }

  setTheme(theme) {
    this.theme = theme;
    document.documentElement.setAttribute('data-theme', theme);
    localStorage.setItem('ruview-theme', theme);
    
    const themeIcon = document.querySelector('.theme-icon');
    if (themeIcon) {
      themeIcon.textContent = theme === 'light' ? '🌙' : '☀️';
    }
  }

  // ========================================
  // VERİ DIŞA AKTARMA
  // ========================================
  initExportModal() {
    const exportBtn = document.getElementById('exportData');
    if (!exportBtn) return;
    
    // Modal oluştur
    this.createExportModal();
    
    exportBtn.addEventListener('click', () => {
      this.showExportModal();
    });
  }

  createExportModal() {
    const modal = document.createElement('div');
    modal.id = 'exportModal';
    modal.className = 'export-modal';
    modal.innerHTML = `
      <div class="export-modal-content">
        <div class="export-modal-header">
          <h3>Veri Dışa Aktarma</h3>
          <button class="export-modal-close">&times;</button>
        </div>
        <div class="export-options">
          <div class="export-option" data-format="csv">
            <span class="export-option-icon">📊</span>
            <div class="export-option-text">
              <h4>CSV Formatı</h4>
              <p>Poz verilerini tablo olarak indir</p>
            </div>
          </div>
          <div class="export-option" data-format="json">
            <span class="export-option-icon">📋</span>
            <div class="export-option-text">
              <h4>JSON Formatı</h4>
              <p>Tam veri yapısını indir</p>
            </div>
          </div>
          <div class="export-option" data-format="image">
            <span class="export-option-icon">🖼️</span>
            <div class="export-option-text">
              <h4>Görüntü</h4>
              <p>Görselleştirmeyi PNG olarak kaydet</p>
            </div>
          </div>
        </div>
      </div>
    `;
    
    document.body.appendChild(modal);
    
    // Kapatma
    modal.querySelector('.export-modal-close').addEventListener('click', () => {
      this.hideExportModal();
    });
    
    modal.addEventListener('click', (e) => {
      if (e.target === modal) this.hideExportModal();
    });
    
    // Format seçimi
    modal.querySelectorAll('.export-option').forEach(option => {
      option.addEventListener('click', () => {
        this.exportData(option.dataset.format);
        this.hideExportModal();
      });
    });
  }

  showExportModal() {
    const modal = document.getElementById('exportModal');
    if (modal) modal.classList.add('active');
  }

  hideExportModal() {
    const modal = document.getElementById('exportModal');
    if (modal) modal.classList.remove('active');
  }

  exportData(format) {
    const data = {
      timestamp: new Date().toISOString(),
      pose: this.currentPose,
      vitalSigns: {
        breathing: this.vitalData.breathing.slice(-50),
        heart: this.vitalData.heart.slice(-50)
      },
      heatmap: this.heatmapData
    };
    
    let content, filename, type;
    
    switch (format) {
      case 'csv':
        content = this.convertToCSV(data);
        filename = 'ruview-data.csv';
        type = 'text/csv';
        break;
      case 'json':
        content = JSON.stringify(data, null, 2);
        filename = 'ruview-data.json';
        type = 'application/json';
        break;
      case 'image':
        this.exportAsImage();
        return;
    }
    
    const blob = new Blob([content], { type });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  }

  convertToCSV(data) {
    let csv = 'Zaman,Nefes,Kalp\n';
    const breathing = data.vitalSigns.breathing;
    const heart = data.vitalSigns.heart;
    
    for (let i = 0; i < Math.max(breathing.length, heart.length); i++) {
      csv += `${i},${breathing[i] || ''},${heart[i] || ''}\n`;
    }
    
    return csv;
  }

  exportAsImage() {
    const canvas = document.getElementById('stickmanCanvas');
    if (!canvas) return;
    
    const link = document.createElement('a');
    link.download = 'ruview-pose.png';
    link.href = canvas.toDataURL();
    link.click();
  }

  // Clean up
  dispose() {
    if (this.healthSubscription) {
      this.healthSubscription();
    }
    if (this._sensingUnsub) this._sensingUnsub();
    if (this._sensingDataUnsub) this._sensingDataUnsub();

    if (this.statsInterval) {
      clearInterval(this.statsInterval);
    }
    if (this.heatmapInterval) {
      clearInterval(this.heatmapInterval);
    }
    if (this.vitalInterval) {
      clearInterval(this.vitalInterval);
    }
    if (this.stickmanInterval) {
      clearInterval(this.stickmanInterval);
    }

    healthService.stopHealthMonitoring();
  }
}