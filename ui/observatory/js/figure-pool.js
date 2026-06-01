/**
 * FigurePool — Manages a pool of human figures for multi-person rendering.
 *
 * Extracted from main.js Observatory class. Owns the lifecycle of up to MAX_FIGURES
 * Three.js figure groups, each containing joints, bones, body segments, and aura.
 *
 * Improvements over the original inline implementation:
 * - Smooth joint interpolation (lerp toward target instead of snapping)
 * - Joint pulsation synced with breathing
 * - Natural bone thickness taper (thicker at shoulder/hip, thinner at extremities)
 * - Secondary motion with slight delay/overshoot for organic feel
 * - Pose-adaptive aura shape (wider for exercise, narrower for crouching)
 *
 * ── HONESTY NOTE (important) ─────────────────────────────────────────────
 * The "capsule" body fill renders an INFERRED, smoothed volumetric silhouette
 * interpolated between ESTIMATED keypoints. Single-antenna WiFi sensing CANNOT
 * measure a person's body surface — there is no camera and no depth scan here.
 * Limb/torso thickness is driven by ONE coarse per-person `bodyScale` scalar
 * (see _estimateBodyScale), never per-pixel geometry. This is a plausible
 * approximation for visualization only. It must never be presented as a
 * "scan", "measured surface", or "camera-accurate" body. The HUD shows an
 * always-visible "inferred — not a camera" disclaimer whenever a body renders.
 * ─────────────────────────────────────────────────────────────────────────
 */
import * as THREE from 'three';

// THREE.CapsuleGeometry exists from r140+ (this app uses r160). We still keep a
// cylinder+sphere-caps fallback so the silhouette degrades gracefully if a
// stripped/older THREE build is ever swapped in. No new dependency is added.
const HAS_CAPSULE = typeof THREE.CapsuleGeometry === 'function';

// 17-keypoint COCO skeleton connectivity
export const SKELETON_PAIRS = [
  [0, 1], [0, 2], [1, 3], [2, 4],
  [5, 6], [5, 7], [7, 9], [6, 8], [8, 10],
  [5, 11], [6, 12], [11, 12],
  [11, 13], [13, 15], [12, 14], [14, 16],
];

// Body segment cylinders that give volume to the wireframe
export const BODY_SEGMENT_DEFS = [
  { joints: [5, 11], radius: 0.12 },   // left torso
  { joints: [6, 12], radius: 0.12 },   // right torso
  { joints: [5, 6], radius: 0.1 },     // shoulder bar
  { joints: [11, 12], radius: 0.1 },   // hip bar
  { joints: [5, 7], radius: 0.05 },    // left upper arm
  { joints: [6, 8], radius: 0.05 },    // right upper arm
  { joints: [7, 9], radius: 0.04 },    // left forearm
  { joints: [8, 10], radius: 0.04 },   // right forearm
  { joints: [11, 13], radius: 0.07 },  // left thigh
  { joints: [12, 14], radius: 0.07 },  // right thigh
  { joints: [13, 15], radius: 0.05 },  // left shin
  { joints: [14, 16], radius: 0.05 },  // right shin
  { joints: [0, 0], radius: 0.1, isHead: true },
];

/**
 * Capsule silhouette segments — the FILLED, inferred body fill ('capsule' mode).
 *
 * Each limb is rendered as a tapered capsule (rounded ends so joints read as
 * smooth blends instead of hard cylinder caps). The torso is built from a few
 * overlapping fuller capsules so it reads as one rounded volume rather than thin
 * bars. `radius` is a BASE half-width in metres; the per-person `bodyScale`
 * scalar (a single coarse number — see _estimateBodyScale) multiplies every
 * radius uniformly so the whole silhouette widens/narrows together. This is an
 * inferred approximation, NOT a per-vertex body measurement.
 */
export const CAPSULE_SEGMENT_DEFS = [
  // ── Torso: overlapping fuller volumes for a rounded, filled core ──
  { joints: [5, 11], radius: 0.155, kind: 'torso' },  // left trunk
  { joints: [6, 12], radius: 0.155, kind: 'torso' },  // right trunk
  { joints: [5, 6],  radius: 0.140, kind: 'torso' },  // chest / shoulder span
  { joints: [11, 12], radius: 0.135, kind: 'torso' }, // pelvis span
  // diagonal cross-fills smooth the trunk into one mass
  { joints: [5, 12], radius: 0.125, kind: 'torso' },
  { joints: [6, 11], radius: 0.125, kind: 'torso' },
  // ── Neck (head ↔ shoulder centre handled in update) ──
  { joints: [0, 5], radius: 0.055, kind: 'neck' },
  { joints: [0, 6], radius: 0.055, kind: 'neck' },
  // ── Arms ──
  { joints: [5, 7], radius: 0.072, kind: 'limb' },   // left upper arm
  { joints: [6, 8], radius: 0.072, kind: 'limb' },   // right upper arm
  { joints: [7, 9], radius: 0.055, kind: 'limb' },   // left forearm
  { joints: [8, 10], radius: 0.055, kind: 'limb' },  // right forearm
  // ── Legs ──
  { joints: [11, 13], radius: 0.098, kind: 'limb' }, // left thigh
  { joints: [12, 14], radius: 0.098, kind: 'limb' }, // right thigh
  { joints: [13, 15], radius: 0.072, kind: 'limb' }, // left shin
  { joints: [14, 16], radius: 0.072, kind: 'limb' }, // right shin
  // ── Head (rounded volume, no face / no features — coarse only) ──
  { joints: [0, 0], radius: 0.125, isHead: true, kind: 'head' },
];

// Bone thickness multipliers — thicker at torso, thinner at extremities
const BONE_TAPER = (() => {
  const tapers = new Map();
  // Torso and shoulder/hip connections are thickest
  tapers.set('5-6', 1.4);    // shoulder bar
  tapers.set('11-12', 1.3);  // hip bar
  tapers.set('5-11', 1.3);   // left torso
  tapers.set('6-12', 1.3);   // right torso
  // Upper limbs
  tapers.set('5-7', 1.0);    // left upper arm
  tapers.set('6-8', 1.0);    // right upper arm
  tapers.set('11-13', 1.1);  // left thigh
  tapers.set('12-14', 1.1);  // right thigh
  // Lower limbs / extremities — thinnest
  tapers.set('7-9', 0.7);    // left forearm
  tapers.set('8-10', 0.7);   // right forearm
  tapers.set('13-15', 0.8);  // left shin
  tapers.set('14-16', 0.8);  // right shin
  // Head connections
  tapers.set('0-1', 0.5);
  tapers.set('0-2', 0.5);
  tapers.set('1-3', 0.4);
  tapers.set('2-4', 0.4);
  return tapers;
})();

// Secondary motion delay factors per joint — extremities lag more
const SECONDARY_DELAY = [
  0.12, // 0 nose
  0.10, // 1 left eye
  0.10, // 2 right eye
  0.08, // 3 left ear
  0.08, // 4 right ear
  0.18, // 5 left shoulder
  0.18, // 6 right shoulder
  0.14, // 7 left elbow
  0.14, // 8 right elbow
  0.10, // 9 left wrist (most lag)
  0.10, // 10 right wrist
  0.20, // 11 left hip (anchored, fast follow)
  0.20, // 12 right hip
  0.15, // 13 left knee
  0.15, // 14 right knee
  0.10, // 15 left ankle
  0.10, // 16 right ankle
];

// Overshoot factors — extremities overshoot more for organic feel
const OVERSHOOT = [
  0.02, // 0 nose
  0.01, // 1 left eye
  0.01, // 2 right eye
  0.01, // 3 left ear
  0.01, // 4 right ear
  0.03, // 5 left shoulder
  0.03, // 6 right shoulder
  0.05, // 7 left elbow
  0.05, // 8 right elbow
  0.08, // 9 left wrist
  0.08, // 10 right wrist
  0.02, // 11 left hip
  0.02, // 12 right hip
  0.04, // 13 left knee
  0.04, // 14 right knee
  0.06, // 15 left ankle
  0.06, // 16 right ankle
];

const MAX_FIGURES = 4;

// Nominal segment length the capsule geometry is authored at. At runtime each
// capsule is scaled along Z to span its two joints, so caps stay roughly rounded.
const CAPSULE_NOMINAL_LEN = 0.35;

// Reusable vectors to avoid per-frame allocation
const _vecFrom = new THREE.Vector3();
const _vecTo = new THREE.Vector3();
const _vecTarget = new THREE.Vector3();
const _vecMid = new THREE.Vector3();
const _vecShoulderMid = new THREE.Vector3();

/**
 * Build a capsule (or cylinder + sphere-cap fallback) oriented along +Z with its
 * origin at the START joint, so it can be placed at jointA, scaled in Z to the
 * joint distance, and aimed with lookAt(jointB) — exactly like the existing
 * bone/segment convention. Returns a THREE.Mesh.
 *
 * @param {number} radius - base half-width in metres
 * @param {THREE.Material} mat
 */
function buildOrientedCapsule(radius, mat) {
  const len = CAPSULE_NOMINAL_LEN;
  let geo;
  if (HAS_CAPSULE) {
    // CapsuleGeometry is authored along +Y, centred at origin, with `length`
    // being the straight mid-section between the two hemispherical caps.
    geo = new THREE.CapsuleGeometry(radius, len, 6, 12);
  } else {
    // Fallback: a cylinder we then cap with spheres (merged visually by overlap).
    geo = new THREE.CylinderGeometry(radius, radius, len, 12, 1, false);
  }
  // Move so the segment spans 0..len in +Y, then reorient to +Z (matches bones).
  geo.translate(0, len / 2, 0);
  geo.rotateX(Math.PI / 2);
  const mesh = new THREE.Mesh(geo, mat);

  if (!HAS_CAPSULE) {
    // Rounded end-caps for the fallback path so joints still read smoothly.
    const capGeo = new THREE.SphereGeometry(radius, 10, 10);
    const capA = new THREE.Mesh(capGeo, mat);
    const capB = new THREE.Mesh(capGeo, mat);
    capA.position.set(0, 0, 0);
    capB.position.set(0, 0, len);
    mesh.add(capA, capB);
    mesh._fallbackCaps = [capA, capB];
  }
  return mesh;
}

export class FigurePool {
  /**
   * @param {THREE.Scene} scene - The Three.js scene to add figures to
   * @param {object} settings - Shared settings object (boneThick, jointSize, glow, etc.)
   * @param {object} poseSystem - PoseSystem instance with generateKeypoints(person, elapsed, breathPulse)
   */
  constructor(scene, settings, poseSystem) {
    this._scene = scene;
    this._settings = settings;
    this._poseSystem = poseSystem;
    this._figures = [];
    this._maxFigures = MAX_FIGURES;
    // bodyFill: 'capsule' (default, inferred filled silhouette) or 'skeleton'
    // (the original thin stick-figure look). Old view stays reachable.
    if (this._settings.bodyFill == null) this._settings.bodyFill = 'capsule';
    this._build();
  }

  /** @returns {boolean} true when the filled capsule silhouette is active */
  get _capsuleMode() { return this._settings.bodyFill !== 'skeleton'; }

  /** @returns {Array} The array of figure objects */
  get figures() { return this._figures; }

  // ---- Construction ----

  _build() {
    for (let f = 0; f < this._maxFigures; f++) {
      this._figures.push(this._createFigure());
    }
  }

  _createFigure() {
    const group = new THREE.Group();
    this._scene.add(group);
    const wireColor = new THREE.Color(this._settings.wireColor);
    const jointColor = new THREE.Color(this._settings.jointColor);

    // Joints (17 COCO keypoints)
    const joints = [];
    for (let i = 0; i < 17; i++) {
      const isNose = i === 0;
      const size = isNose ? this._settings.jointSize * 0.7 : this._settings.jointSize;
      const geo = new THREE.SphereGeometry(size, 12, 12);
      const mat = new THREE.MeshStandardMaterial({
        color: isNose ? wireColor : jointColor,
        emissive: isNose ? wireColor : jointColor,
        emissiveIntensity: 0.35,
        transparent: true, opacity: 0,
        roughness: 0.3, metalness: 0.2,
      });
      const sphere = new THREE.Mesh(geo, mat);
      sphere.castShadow = true;
      group.add(sphere);
      joints.push(sphere);

      // Halo glow on key joints
      if ([5, 6, 9, 10, 11, 12, 15, 16].includes(i)) {
        const haloGeo = new THREE.SphereGeometry(size * 1.3, 8, 8);
        const haloMat = new THREE.MeshBasicMaterial({
          color: jointColor,
          transparent: true, opacity: 0,
          blending: THREE.AdditiveBlending,
          depthWrite: false,
        });
        const halo = new THREE.Mesh(haloGeo, haloMat);
        sphere.add(halo);
        sphere._halo = halo;
        sphere._haloMat = haloMat;

        const glow = new THREE.PointLight(jointColor, 0, 0.8);
        sphere.add(glow);
        sphere._glow = glow;
      }
    }

    // Bones — tapered thickness
    const bones = [];
    for (const [a, b] of SKELETON_PAIRS) {
      const taperKey = `${Math.min(a, b)}-${Math.max(a, b)}`;
      const taper = BONE_TAPER.get(taperKey) || 1.0;
      const thick = this._settings.boneThick * taper;
      // Top radius thicker than bottom for natural taper along bone length
      const topRadius = thick;
      const botRadius = thick * 0.65;
      const geo = new THREE.CylinderGeometry(topRadius, botRadius, 1, 8, 1);
      geo.translate(0, 0.5, 0);
      geo.rotateX(Math.PI / 2);
      const mat = new THREE.MeshStandardMaterial({
        color: wireColor, emissive: wireColor, emissiveIntensity: 0.3,
        transparent: true, opacity: 0, roughness: 0.4, metalness: 0.1,
      });
      const mesh = new THREE.Mesh(geo, mat);
      mesh.castShadow = true;
      group.add(mesh);
      bones.push({ mesh, a, b, taper });
    }

    // Body segments (volume cylinders and head sphere)
    const bodySegments = [];
    for (const seg of BODY_SEGMENT_DEFS) {
      const geo = seg.isHead
        ? new THREE.SphereGeometry(seg.radius, 12, 12)
        : new THREE.CylinderGeometry(seg.radius, seg.radius * 0.85, 1, 8, 1);
      if (!seg.isHead) {
        geo.translate(0, 0.5, 0);
        geo.rotateX(Math.PI / 2);
      }
      const mat = new THREE.MeshStandardMaterial({
        color: wireColor, emissive: wireColor, emissiveIntensity: 0.12,
        transparent: true, opacity: 0, roughness: 0.5, metalness: 0.1,
        side: THREE.DoubleSide,
      });
      const mesh = new THREE.Mesh(geo, mat);
      group.add(mesh);
      bodySegments.push({ mesh, mat, a: seg.joints[0], b: seg.joints[1], isHead: seg.isHead });
    }

    // ── Capsule silhouette ('capsule' fill) ──
    // The FILLED, inferred body: tapered capsules for limbs, fuller overlapping
    // capsules for the torso, a rounded head sphere. Thickness is uniformly
    // scaled at runtime by a single coarse per-person `bodyScale` scalar.
    const capsules = [];
    for (const seg of CAPSULE_SEGMENT_DEFS) {
      const mat = new THREE.MeshStandardMaterial({
        color: wireColor, emissive: wireColor,
        // Low emissive so the body reads by SHAPE (lit volume), not by glow.
        // Over-bright emissive previously washed every figure into a green blob.
        emissiveIntensity: seg.kind === 'torso' ? 0.06 : 0.08,
        transparent: true, opacity: 0,
        roughness: 0.55, metalness: 0.05,
        // DoubleSide is ESSENTIAL: figures can face any direction (e.g. the two
        // walkers face opposite ways). With the default FrontSide, a figure
        // facing away culls its front faces and the capsule renders invisible —
        // which is why one walker looked like a filled body and the other like
        // a bare stick. DoubleSide makes every figure render identically.
        side: THREE.DoubleSide,
        // Write depth so the body reads solid regardless of transparent-sort.
        depthWrite: true,
      });
      let mesh;
      if (seg.isHead) {
        mesh = new THREE.Mesh(new THREE.SphereGeometry(seg.radius, 16, 16), mat);
      } else {
        mesh = buildOrientedCapsule(seg.radius, mat);
      }
      mesh.castShadow = true;
      // Draw the solid body before the additive aura/halos (renderOrder 0)
      // so the silhouette is never overwritten by another figure's glow.
      mesh.renderOrder = 1;
      group.add(mesh);
      capsules.push({
        mesh, mat,
        a: seg.joints[0], b: seg.joints[1],
        baseRadius: seg.radius, kind: seg.kind, isHead: !!seg.isHead,
      });
    }

    // Aura cylinder
    const auraGeo = new THREE.CylinderGeometry(0.4, 0.3, 1.7, 16, 1, true);
    const auraMat = new THREE.MeshBasicMaterial({
      color: wireColor, transparent: true, opacity: 0,
      side: THREE.DoubleSide, blending: THREE.AdditiveBlending, depthWrite: false,
    });
    const aura = new THREE.Mesh(auraGeo, auraMat);
    aura.position.y = 1;
    // Draw the faint halo AFTER the solid body so it stays a subtle outer haze
    // and never fills over the silhouette.
    aura.renderOrder = 2;
    group.add(aura);

    // Per-figure point light
    const personLight = new THREE.PointLight(wireColor, 0, 6);
    personLight.position.y = 1;
    group.add(personLight);

    // Interpolation state: previous positions for smooth lerp and secondary motion
    const prevPositions = [];
    const velocities = [];
    for (let i = 0; i < 17; i++) {
      prevPositions.push(new THREE.Vector3(0, 0, 0));
      velocities.push(new THREE.Vector3(0, 0, 0));
    }

    return {
      group, joints, bones, bodySegments, capsules, aura, auraMat, personLight,
      visible: false,
      prevPositions,
      velocities,
      _initialized: false,
      _lastPose: null,
      // Smoothed coarse body-size scalar (inferred, not measured). 1.0 = average build.
      bodyScale: 1.0,
      // Smoothed confidence used to size the uncertainty aura. Starts uncertain.
      confSmooth: 0.5,
    };
  }

  // ---- Per-frame update ----

  /**
   * Update all figures based on current data frame.
   * @param {object} data - Current sensing data with persons[], vital_signs, classification
   * @param {number} elapsed - Elapsed time in seconds
   */
  update(data, elapsed) {
    const persons = data?.persons || [];
    const vs = data?.vital_signs || {};
    const cls = data?.classification || {};
    const isPresent = cls.presence || false;
    // Global sensing confidence (0..1) — single-antenna WiFi gives us a coarse
    // confidence, not a per-person measured one, so all bodies share it. Drives
    // the uncertainty aura: low confidence ⇒ wider, hazier halo.
    const confidence = typeof cls.confidence === 'number' ? cls.confidence : 0.5;
    const breathBpm = vs.breathing_rate_bpm || 0;
    const breathPulse = breathBpm > 0
      ? Math.sin(elapsed * Math.PI * 2 * (breathBpm / 60)) * 0.012
      : 0;

    for (let f = 0; f < this._figures.length; f++) {
      const fig = this._figures[f];
      if (f < persons.length && isPresent) {
        const p = persons[f];
        const kps = this._poseSystem.generateKeypoints(p, elapsed, breathPulse);
        this.applyKeypoints(fig, kps, breathPulse, p.position || [0, 0, 0], elapsed, p.pose, p, confidence);
        fig.visible = true;
      } else {
        if (fig.visible) {
          this.hide(fig);
          fig.visible = false;
        }
      }
    }
  }

  /**
   * Apply keypoints to a figure with smooth interpolation, pulsation, and secondary motion.
   * @param {object} fig - Figure object from the pool
   * @param {Array} kps - 17-element array of [x,y,z] keypoint positions
   * @param {number} breathPulse - Current breathing pulse value
   * @param {Array} pos - Person world position [x,y,z]
   * @param {number} elapsed - Elapsed time for pulsation effects
   * @param {string} pose - Current pose name for aura adaptation
   */
  applyKeypoints(fig, kps, breathPulse, pos, elapsed = 0, pose = 'standing', person = null, confidence = 0.5) {
    const lerpFactor = fig._initialized ? 0.18 : 1.0;
    const capsuleMode = this._capsuleMode;

    // Smooth the coarse, inferred body-size scalar so the silhouette doesn't
    // jitter as noisy keypoints wobble. ONE number sizes the whole body.
    const targetScale = this._estimateBodyScale(kps, person);
    fig.bodyScale += (targetScale - fig.bodyScale) * (fig._initialized ? 0.05 : 1.0);
    // Smooth confidence too (drives the uncertainty aura).
    fig.confSmooth += (confidence - fig.confSmooth) * (fig._initialized ? 0.06 : 1.0);

    // Joints with smooth interpolation and secondary motion
    for (let i = 0; i < 17 && i < kps.length; i++) {
      const j = fig.joints[i];
      _vecTarget.set(kps[i][0], kps[i][1], kps[i][2]);

      if (fig._initialized) {
        // Compute velocity for overshoot
        const prev = fig.prevPositions[i];
        const vel = fig.velocities[i];

        // Smooth lerp with per-joint delay
        const delay = SECONDARY_DELAY[i];
        const jointLerp = lerpFactor + delay;
        j.position.lerp(_vecTarget, Math.min(jointLerp, 0.95));

        // Apply subtle overshoot based on velocity change
        const overshoot = OVERSHOOT[i];
        vel.subVectors(j.position, prev).multiplyScalar(overshoot);
        j.position.add(vel);

        prev.copy(j.position);
      } else {
        // First frame: snap to position
        j.position.copy(_vecTarget);
        fig.prevPositions[i].copy(_vecTarget);
        fig.velocities[i].set(0, 0, 0);
      }

      // In capsule mode the red joint dots are subtle accents on the volume,
      // not the primary read — keep them dimmer and smaller so the silhouette
      // dominates. In skeleton mode they stay as before.
      j.material.opacity = capsuleMode ? 0.6 : 0.95;

      // Joint pulsation synced with breathing
      const pulseFactor = 1.0 + Math.abs(breathPulse) * 8.0;
      j.material.emissiveIntensity = (capsuleMode ? 0.2 : 0.35) * pulseFactor;

      const baseScale = (this._settings.jointSize / 0.04) * (capsuleMode ? 0.6 : 1.0);
      // Subtle size pulsation on breathing
      const pulseScale = baseScale * (1.0 + Math.abs(breathPulse) * 3.0);
      j.scale.setScalar(pulseScale);

      if (j._haloMat) {
        // Halo glow further muted in capsule mode so it doesn't add to the bloom.
        j._haloMat.opacity = (capsuleMode ? 0.012 : 0.04) * this._settings.glow * pulseFactor;
      }
      if (j._glow) {
        j._glow.intensity = (capsuleMode ? 0.05 : 0.12) * this._settings.glow * pulseFactor;
      }
    }

    fig._initialized = true;

    // Bones with tapered thickness
    for (const bone of fig.bones) {
      const pA = kps[bone.a], pB = kps[bone.b];
      if (pA && pB) {
        _vecFrom.set(pA[0], pA[1], pA[2]);
        _vecTo.set(pB[0], pB[1], pB[2]);
        const len = _vecFrom.distanceTo(_vecTo);

        // Use interpolated joint positions for smooth bone movement
        if (fig._initialized) {
          const jA = fig.joints[bone.a];
          const jB = fig.joints[bone.b];
          bone.mesh.position.copy(jA.position);
          bone.mesh.scale.set(1, 1, jA.position.distanceTo(jB.position));
          bone.mesh.lookAt(jB.position);
        } else {
          bone.mesh.position.copy(_vecFrom);
          bone.mesh.scale.set(1, 1, len);
          bone.mesh.lookAt(_vecTo);
        }

        // In capsule mode the thin bones become faint internal scaffolding so
        // the filled silhouette dominates; in skeleton mode they stay bright.
        bone.mesh.material.opacity = capsuleMode ? 0.18 : 0.85;
        bone.mesh.material.emissiveIntensity = (capsuleMode ? 0.15 : 0.3) + Math.abs(breathPulse) * 2.0;
      }
    }

    // Body segments (the original thin volume cylinders). Keep them as subtle
    // inner mass under the capsules in capsule mode; brighter as the volume in
    // skeleton mode.
    for (const seg of fig.bodySegments) {
      if (seg.isHead) {
        const headJoint = fig.joints[seg.a];
        seg.mesh.position.set(headJoint.position.x, headJoint.position.y + 0.05, headJoint.position.z);
        seg.mat.opacity = capsuleMode ? 0.0 : 0.15;
      } else {
        const jA = fig.joints[seg.a];
        const jB = fig.joints[seg.b];
        if (jA && jB) {
          const len = jA.position.distanceTo(jB.position);
          seg.mesh.position.copy(jA.position);
          seg.mesh.scale.set(1, 1, len);
          seg.mesh.lookAt(jB.position);
          seg.mat.opacity = capsuleMode ? 0.06 : 0.12;
        }
      }
      seg.mat.emissiveIntensity = 0.1 + Math.abs(breathPulse) * 0.4;
    }

    // ── Capsule silhouette: the inferred, filled body fill ──
    this._updateCapsules(fig, capsuleMode, breathPulse);

    // ── Uncertainty aura ──
    // Width and opacity scale with (1 - confidence): low confidence renders a
    // wider, hazier halo (honest "we're not sure exactly where the body is");
    // high confidence tightens it toward the silhouette.
    const hipY = (fig.joints[11].position.y + fig.joints[12].position.y) / 2;
    const cx = (fig.joints[11].position.x + fig.joints[12].position.x) / 2;
    const cz = (fig.joints[11].position.z + fig.joints[12].position.z) / 2;
    fig.aura.position.set(cx, hipY, cz);

    const uncertainty = 1 - Math.max(0, Math.min(1, fig.confSmooth)); // 0=sure, 1=unsure
    // Base aura opacity grows with uncertainty (hazier when unsure), but is
    // HARD-CAPPED low so it stays a faint additive halo around the silhouette
    // instead of a solid green fill that hides the body contours.
    const auraOpacity =
      this._settings.aura * (0.5 + uncertainty * 1.6) + Math.abs(breathPulse) * 0.15;
    fig.auraMat.opacity = Math.min(auraOpacity, 0.07);

    // Pose-adaptive aura shape, then expanded outward by uncertainty + body scale.
    const auraShape = this._computeAuraShape(fig, pose, breathPulse);
    const haze = 1 + uncertainty * 0.9;                 // up to ~1.9x wider when unsure
    const widthScale = fig.bodyScale * haze;
    fig.aura.scale.set(
      auraShape.scaleX * widthScale,
      auraShape.scaleY * (1 + uncertainty * 0.12),
      auraShape.scaleZ * widthScale,
    );

    // Person light — kept modest so several figures don't flood the scene with
    // green fill (which contributed to the washed-out "blob" look).
    fig.personLight.position.set(pos[0], 1.2, pos[2]);
    fig.personLight.intensity = this._settings.glow * 0.25;

    fig._lastPose = pose;
  }

  /**
   * Estimate a SINGLE coarse body-size scalar (≈0.8 slim … ≈1.3 large) for a
   * person. This is an INFERRED approximation — WiFi cannot measure body width.
   * We blend a few weak cues:
   *   - explicit size/build hint in the data, if present (data may not have one)
   *   - shoulder span vs an average reference (noisy but cheap)
   *   - motion_score as a faint proxy (larger movers read slightly fuller)
   * The result uniformly widens/narrows the whole silhouette — never per-limb,
   * never per-pixel. Defaults to an average build when cues are weak.
   *
   * @param {Array} kps - 17 keypoints [x,y,z]
   * @param {object|null} person - per-person data (may carry size/build/motion_score)
   * @returns {number} coarse uniform thickness scalar
   */
  _estimateBodyScale(kps, person) {
    // 1) Explicit hint wins if the data provides one (kept honest: still coarse).
    if (person) {
      const hint = person.bodyScale ?? person.size ?? person.build;
      if (typeof hint === 'number' && isFinite(hint)) {
        return Math.max(0.75, Math.min(1.35, hint));
      }
    }

    // 2) Shoulder span relative to an average adult reference (~0.42 m).
    let spanRatio = 1.0;
    const lS = kps?.[5], rS = kps?.[6];
    if (lS && rS) {
      const dx = rS[0] - lS[0];
      const dz = rS[2] - lS[2];
      const span = Math.sqrt(dx * dx + dz * dz);
      if (span > 0.05) spanRatio = span / 0.42;
    }

    // 3) Faint motion proxy — bigger movers read very slightly fuller.
    const ms = person && typeof person.motion_score === 'number' ? person.motion_score : 0;
    const motionBias = Math.max(0, Math.min(0.08, ms / 600)); // ≤ +0.08

    // Weighted, heavily damped toward 1.0 (average build) since cues are weak.
    const raw = 0.78 + spanRatio * 0.20 + motionBias;
    return Math.max(0.8, Math.min(1.3, raw));
  }

  /**
   * Drive the capsule silhouette each frame from interpolated joint positions.
   * Thickness = baseRadius × bodyScale (one coarse scalar), with a gentle
   * breathing swell on the torso. The neck capsules bridge head→shoulder centre.
   *
   * @param {object} fig
   * @param {boolean} capsuleMode - false hides capsules (skeleton fill active)
   * @param {number} breathPulse
   */
  _updateCapsules(fig, capsuleMode, breathPulse) {
    if (!fig.capsules) return;
    const scale = fig.bodyScale;
    const breathSwell = 1 + Math.abs(breathPulse) * 4.0; // subtle chest rise

    // Shoulder centre, reused for the neck capsules.
    _vecShoulderMid.copy(fig.joints[5].position).add(fig.joints[6].position).multiplyScalar(0.5);

    for (const cap of fig.capsules) {
      const mat = cap.mat;
      if (!capsuleMode) { mat.opacity = 0; continue; }

      if (cap.isHead) {
        const head = fig.joints[cap.a].position;
        cap.mesh.position.set(head.x, head.y + 0.04, head.z);
        // Head widens with body scale but never gains features — coarse sphere only.
        cap.mesh.scale.setScalar(scale);
        // Translucent, low-glow so the head reads as a rounded volume, not a bulb.
        mat.opacity = 0.55;
        mat.emissiveIntensity = 0.07 + Math.abs(breathPulse) * 0.25;
        continue;
      }

      // Resolve endpoints. Neck capsules go head(0) → shoulder centre.
      const aPos = fig.joints[cap.a].position;
      let bx, by, bz;
      if (cap.kind === 'neck') {
        bx = _vecShoulderMid.x; by = _vecShoulderMid.y; bz = _vecShoulderMid.z;
      } else {
        const bPos = fig.joints[cap.b].position;
        bx = bPos.x; by = bPos.y; bz = bPos.z;
      }
      _vecMid.set(bx, by, bz);
      const len = aPos.distanceTo(_vecMid);
      if (len < 1e-4) { mat.opacity = 0; continue; }

      cap.mesh.position.copy(aPos);
      // Capsule authored at CAPSULE_NOMINAL_LEN along +Z, origin at start joint.
      // X/Y scale = radial thickness (bodyScale); Z scale = span / nominal length.
      const radial = scale * (cap.kind === 'torso' ? breathSwell : 1);
      cap.mesh.scale.set(radial, radial, len / CAPSULE_NOMINAL_LEN);
      cap.mesh.lookAt(_vecMid);

      // Translucent body: torso a touch more solid than limbs so you can read
      // the rounded core, while limbs keep a soft, see-through edge. Low values
      // (vs the old ~0.9) stop the figure saturating into a green blob — the
      // shape is carried by lighting + contour, not by brightness.
      mat.opacity = cap.kind === 'torso' ? 0.55 : 0.5;
      // Very low emissive; depth/normal lighting now defines limb/torso shape.
      mat.emissiveIntensity = (cap.kind === 'torso' ? 0.06 : 0.08) + Math.abs(breathPulse) * 0.3;
    }
  }

  /**
   * Compute pose-adaptive aura shape based on actual keypoint spread.
   * Wider for exercise/spread poses, narrower for crouching/compact poses.
   */
  _computeAuraShape(fig, pose, breathPulse) {
    // Measure horizontal spread from shoulders and hips
    const lShoulder = fig.joints[5].position;
    const rShoulder = fig.joints[6].position;
    const lHip = fig.joints[11].position;
    const rHip = fig.joints[12].position;
    const nose = fig.joints[0].position;
    const lAnkle = fig.joints[15].position;
    const rAnkle = fig.joints[16].position;

    // Horizontal spread (X-Z plane)
    const shoulderWidth = Math.sqrt(
      (rShoulder.x - lShoulder.x) ** 2 +
      (rShoulder.z - lShoulder.z) ** 2
    );
    const ankleWidth = Math.sqrt(
      (rAnkle.x - lAnkle.x) ** 2 +
      (rAnkle.z - lAnkle.z) ** 2
    );
    const maxWidth = Math.max(shoulderWidth, ankleWidth);

    // Vertical extent
    const headY = nose.y;
    const footY = Math.min(lAnkle.y, rAnkle.y);
    const height = headY - footY;

    // Normalize to base aura dimensions
    const baseWidth = 0.44; // default shoulder width
    const baseHeight = 1.7; // default standing height

    const widthRatio = Math.max(0.6, Math.min(2.0, maxWidth / baseWidth));
    const heightRatio = Math.max(0.4, Math.min(1.3, height / baseHeight));

    // Breathing modulation
    const breathMod = 1 + breathPulse * 2;

    return {
      scaleX: widthRatio * breathMod,
      scaleY: heightRatio * breathMod,
      scaleZ: widthRatio * breathMod,
    };
  }

  /**
   * Hide a figure by fading all materials to invisible.
   * @param {object} fig - Figure object to hide
   */
  hide(fig) {
    for (const j of fig.joints) {
      j.material.opacity = 0;
      if (j._haloMat) j._haloMat.opacity = 0;
      if (j._glow) j._glow.intensity = 0;
    }
    for (const b of fig.bones) b.mesh.material.opacity = 0;
    for (const seg of fig.bodySegments) seg.mat.opacity = 0;
    if (fig.capsules) for (const cap of fig.capsules) cap.mat.opacity = 0;
    fig.auraMat.opacity = 0;
    fig.personLight.intensity = 0;
    fig._initialized = false;
  }

  /**
   * Apply wire and joint colors to all figures in the pool.
   * @param {THREE.Color} wireColor
   * @param {THREE.Color} jointColor
   */
  applyColors(wireColor, jointColor) {
    for (const fig of this._figures) {
      for (let i = 0; i < fig.joints.length; i++) {
        const j = fig.joints[i];
        if (i === 0) {
          j.material.color.copy(wireColor);
          j.material.emissive.copy(wireColor);
        } else {
          j.material.color.copy(jointColor);
          j.material.emissive.copy(jointColor);
        }
        if (j._haloMat) j._haloMat.color.copy(jointColor);
        if (j._glow) j._glow.color.copy(jointColor);
      }
      for (const b of fig.bones) {
        b.mesh.material.color.copy(wireColor);
        b.mesh.material.emissive.copy(wireColor);
      }
      for (const seg of fig.bodySegments) {
        seg.mat.color.copy(wireColor);
        seg.mat.emissive.copy(wireColor);
      }
      if (fig.capsules) {
        for (const cap of fig.capsules) {
          cap.mat.color.copy(wireColor);
          cap.mat.emissive.copy(wireColor);
        }
      }
      fig.auraMat.color.copy(wireColor);
      fig.personLight.color.copy(wireColor);
    }
  }
}
