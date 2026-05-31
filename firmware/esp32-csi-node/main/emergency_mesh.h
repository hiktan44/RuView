/**
 * @file emergency_mesh.h
 * @brief Internet-free SoftAP fallback for disaster-response deployments.
 *
 * In a disaster (earthquake, collapse) the building Wi-Fi infrastructure is
 * usually gone. The default firmware connects as a station (STA) to a router —
 * useless when there is no router. Emergency mesh solves this:
 *
 *   1. The node tries to join the provisioned router (normal STA mode).
 *   2. If that fails after the retry budget, it raises its OWN Wi-Fi network
 *      (SoftAP): SSID "RuView-Rescue-<nodeId>", so a rescuer's phone or laptop
 *      connects DIRECTLY to the node — no router, no internet, no cloud.
 *   3. CSI/sensing frames are streamed to whatever client is attached to the
 *      SoftAP (the DHCP-assigned client, typically 192.168.4.2), over the same
 *      UDP port, so the existing pipeline works unchanged.
 *
 * This is the firmware half of the "emergency mesh" feature; the mobile app's
 * discovery service points at the SoftAP gateway (192.168.4.1) to receive data.
 *
 * The SoftAP is opened only as a FALLBACK so normal (router-present) operation
 * is unchanged. Security: the rescue AP is WPA2 with a deployment passphrase
 * from NVS when set, else open (documented trade-off — rescue speed over
 * secrecy; see ADR-032 for the hardened-mesh discussion).
 */

#ifndef EMERGENCY_MESH_H
#define EMERGENCY_MESH_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** SoftAP network defaults. */
#define EMESH_SSID_PREFIX     "RuView-Rescue-"
#define EMESH_AP_CHANNEL      6        /**< 2.4 GHz channel for the rescue AP. */
#define EMESH_AP_MAX_CONN     4        /**< Max rescuer clients on one node. */
#define EMESH_AP_GATEWAY      "192.168.4.1"  /**< esp-idf SoftAP default gateway. */
#define EMESH_DEFAULT_PORT    5005     /**< UDP port for CSI to the connected client. */

/** Operating mode the node settled into. */
typedef enum {
    EMESH_MODE_STA = 0,   /**< Joined the provisioned router (normal). */
    EMESH_MODE_SOFTAP,    /**< Raised its own rescue AP (router unavailable). */
} emesh_mode_t;

/** Runtime state for the emergency mesh subsystem. */
typedef struct {
    emesh_mode_t mode;          /**< Current operating mode. */
    char         ap_ssid[33];   /**< The rescue SSID, when in SoftAP mode. */
    char         client_ip[16]; /**< Last connected rescuer IP (SoftAP mode). */
    bool         client_connected; /**< True while a rescuer is attached. */
    uint8_t      client_count;  /**< Number of attached rescuer clients. */
} emesh_state_t;

/**
 * @brief Bring up the rescue SoftAP for this node.
 *
 * Call this after STA association has definitively failed. Initialises the
 * SoftAP network interface (if not already), configures SSID/password/channel,
 * and starts broadcasting. The node keeps streaming CSI to the connected client.
 *
 * @param node_id   Node identifier, appended to the SSID.
 * @param password  WPA2 passphrase (>= 8 chars) or NULL/empty for an open AP.
 * @param out_state Optional; filled with the resulting mode + SSID.
 * @return 0 on success, negative on error.
 */
int emergency_mesh_start_softap(uint8_t node_id, const char *password,
                                emesh_state_t *out_state);

/**
 * @brief Query current emergency-mesh state (mode, client connection).
 * @param out_state Filled with the latest state. Must not be NULL.
 */
void emergency_mesh_get_state(emesh_state_t *out_state);

/**
 * @brief Resolve the UDP destination IP for streaming in the current mode.
 *
 * In STA mode this returns the provisioned aggregator IP unchanged. In SoftAP
 * mode it returns the connected rescuer client's IP (or the SoftAP broadcast
 * address 192.168.4.255 when no specific client is known yet), so CSI reaches
 * whoever joined the rescue network.
 *
 * @param sta_target_ip  The provisioned aggregator IP (used in STA mode).
 * @param out_ip         Buffer (>= 16 bytes) for the resolved destination.
 * @param out_len        Size of out_ip.
 * @return 0 on success, negative on error.
 */
int emergency_mesh_resolve_target(const char *sta_target_ip, char *out_ip, int out_len);

#ifdef __cplusplus
}
#endif

#endif /* EMERGENCY_MESH_H */
