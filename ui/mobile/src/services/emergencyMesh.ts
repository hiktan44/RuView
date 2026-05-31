/**
 * Emergency-mesh helper for the field rescuer (internet-free operation).
 *
 * When the building Wi-Fi is gone (earthquake, collapse), each ESP32-S3 node
 * falls back to its own rescue SoftAP — SSID `RuView-Rescue-<nodeId>`, gateway
 * 192.168.4.1 (see firmware/esp32-csi-node/main/emergency_mesh.c). The rescuer:
 *
 *   1. Opens phone Wi-Fi settings, joins `RuView-Rescue-<N>`.
 *   2. Opens this app; it points the sensing WebSocket at the node gateway.
 *
 * React Native / Expo cannot programmatically join a Wi-Fi network without a
 * native module, so this module does the realistic part: it produces the right
 * endpoint URL for a node's SoftAP gateway and gives the rescuer clear,
 * copy-ready join instructions. The mobile offline inference engine then keeps
 * deriving presence/vitals even if a node drops out.
 */

/** Default SoftAP gateway exposed by an ESP32-S3 emergency-mesh node. */
export const EMESH_GATEWAY = '192.168.4.1';

/** Default UDP/WS port the node streams CSI on. */
export const EMESH_PORT = 5005;

/** SSID prefix the firmware uses for the rescue AP. */
export const EMESH_SSID_PREFIX = 'RuView-Rescue-';

/** The Wi-Fi network name a given node raises in emergency mode. */
export function rescueSsid(nodeId: number): string {
  return `${EMESH_SSID_PREFIX}${nodeId}`;
}

/**
 * The WebSocket sensing endpoint for a node reached over its rescue SoftAP.
 * All emergency-mesh nodes use the same gateway (192.168.4.1) because the
 * rescuer is on exactly one node's network at a time.
 */
export function rescueWsUrl(host: string = EMESH_GATEWAY, port: number = EMESH_PORT): string {
  return `ws://${host}:${port}/ws/sensing`;
}

/** Step-by-step instructions shown to the rescuer to join a node. */
export function rescueJoinSteps(nodeId: number): string[] {
  const ssid = rescueSsid(nodeId);
  return [
    `Open your phone's Wi-Fi settings.`,
    `Join the network "${ssid}" (no internet — this is the rescue node).`,
    `If asked, the password is the deployment passphrase (or it is open).`,
    `Return to RuView; it connects to the node at ${EMESH_GATEWAY}.`,
    `Survivor vitals appear as the node streams CSI directly to your phone.`,
  ];
}

/** True when a host string looks like an emergency-mesh SoftAP gateway. */
export function isEmergencyMeshHost(host: string): boolean {
  const h = host.trim().replace(/^wss?:\/\//i, '').split(/[:/]/)[0];
  // esp-idf SoftAP subnet is 192.168.4.x with gateway .1
  return h === EMESH_GATEWAY || /^192\.168\.4\.\d{1,3}$/.test(h);
}
