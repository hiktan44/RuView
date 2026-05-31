/**
 * @file emergency_mesh.c
 * @brief Internet-free SoftAP fallback implementation. See emergency_mesh.h.
 */

#include "emergency_mesh.h"

#include <string.h>
#include <stdio.h>

#include "esp_wifi.h"
#include "esp_netif.h"
#include "esp_event.h"
#include "esp_log.h"
#include "esp_mac.h"

static const char *TAG = "emesh";

/* Single-instance runtime state, updated from Wi-Fi event callbacks. */
static emesh_state_t s_state = {
    .mode = EMESH_MODE_STA,
    .ap_ssid = {0},
    .client_ip = {0},
    .client_connected = false,
    .client_count = 0,
};

static esp_netif_t *s_ap_netif = NULL;

/* SoftAP event handler: track rescuer client connect/disconnect + IP. */
static void emesh_event_handler(void *arg, esp_event_base_t base,
                                int32_t id, void *data)
{
    (void)arg;
    if (base == WIFI_EVENT && id == WIFI_EVENT_AP_STACONNECTED) {
        wifi_event_ap_staconnected_t *e = (wifi_event_ap_staconnected_t *)data;
        if (s_state.client_count < 0xFF) {
            s_state.client_count++;
        }
        s_state.client_connected = true;
        ESP_LOGI(TAG, "Rescuer connected: " MACSTR " (aid=%d), %u client(s)",
                 MAC2STR(e->mac), e->aid, (unsigned)s_state.client_count);
    } else if (base == WIFI_EVENT && id == WIFI_EVENT_AP_STADISCONNECTED) {
        wifi_event_ap_stadisconnected_t *e = (wifi_event_ap_stadisconnected_t *)data;
        if (s_state.client_count > 0) {
            s_state.client_count--;
        }
        if (s_state.client_count == 0) {
            s_state.client_connected = false;
            s_state.client_ip[0] = '\0';
        }
        ESP_LOGW(TAG, "Rescuer disconnected: " MACSTR " (aid=%d), %u left",
                 MAC2STR(e->mac), e->aid, (unsigned)s_state.client_count);
    } else if (base == IP_EVENT && id == IP_EVENT_AP_STAIPASSIGNED) {
        ip_event_ap_staipassigned_t *e = (ip_event_ap_staipassigned_t *)data;
        snprintf(s_state.client_ip, sizeof(s_state.client_ip), IPSTR,
                 IP2STR(&e->ip));
        ESP_LOGI(TAG, "Assigned rescuer IP: %s", s_state.client_ip);
    }
}

int emergency_mesh_start_softap(uint8_t node_id, const char *password,
                                emesh_state_t *out_state)
{
    /* Build the rescue SSID: RuView-Rescue-<nodeId>. */
    char ssid[33];
    int n = snprintf(ssid, sizeof(ssid), "%s%u", EMESH_SSID_PREFIX,
                     (unsigned)node_id);
    if (n <= 0 || n >= (int)sizeof(ssid)) {
        ESP_LOGE(TAG, "SSID build failed");
        return -1;
    }

    /* The default WiFi init created an STA netif; add the AP netif. The caller
     * has already run esp_wifi_init(); we only add the AP interface + config.
     * Switch the radio to combined AP+STA so a future re-association is still
     * possible, but the AP is what rescuers actually use. */
    if (s_ap_netif == NULL) {
        s_ap_netif = esp_netif_create_default_wifi_ap();
        if (s_ap_netif == NULL) {
            ESP_LOGE(TAG, "Failed to create AP netif");
            return -2;
        }
    }

    ESP_ERROR_CHECK(esp_event_handler_instance_register(
        WIFI_EVENT, ESP_EVENT_ANY_ID, &emesh_event_handler, NULL, NULL));
    ESP_ERROR_CHECK(esp_event_handler_instance_register(
        IP_EVENT, IP_EVENT_AP_STAIPASSIGNED, &emesh_event_handler, NULL, NULL));

    wifi_config_t ap_cfg = {0};
    strncpy((char *)ap_cfg.ap.ssid, ssid, sizeof(ap_cfg.ap.ssid) - 1);
    ap_cfg.ap.ssid_len = (uint8_t)strlen(ssid);
    ap_cfg.ap.channel = EMESH_AP_CHANNEL;
    ap_cfg.ap.max_connection = EMESH_AP_MAX_CONN;
    ap_cfg.ap.beacon_interval = 100;

    if (password != NULL && strlen(password) >= 8) {
        strncpy((char *)ap_cfg.ap.password, password,
                sizeof(ap_cfg.ap.password) - 1);
        ap_cfg.ap.authmode = WIFI_AUTH_WPA2_PSK;
    } else {
        ap_cfg.ap.authmode = WIFI_AUTH_OPEN;
        ESP_LOGW(TAG, "Rescue AP is OPEN (no passphrase >= 8 chars provided)");
    }

    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_APSTA));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_AP, &ap_cfg));
    ESP_ERROR_CHECK(esp_wifi_start());

    s_state.mode = EMESH_MODE_SOFTAP;
    strncpy(s_state.ap_ssid, ssid, sizeof(s_state.ap_ssid) - 1);
    s_state.ap_ssid[sizeof(s_state.ap_ssid) - 1] = '\0';
    s_state.client_connected = false;
    s_state.client_count = 0;
    s_state.client_ip[0] = '\0';

    ESP_LOGW(TAG, "EMERGENCY MESH active — rescue SSID '%s' on channel %d, "
                  "gateway %s. Connect a phone/laptop to this network.",
             ssid, EMESH_AP_CHANNEL, EMESH_AP_GATEWAY);

    if (out_state != NULL) {
        *out_state = s_state;
    }
    return 0;
}

void emergency_mesh_get_state(emesh_state_t *out_state)
{
    if (out_state != NULL) {
        *out_state = s_state;
    }
}

int emergency_mesh_resolve_target(const char *sta_target_ip, char *out_ip, int out_len)
{
    if (out_ip == NULL || out_len < 16) {
        return -1;
    }

    if (s_state.mode == EMESH_MODE_STA) {
        /* Normal mode: stream to the provisioned aggregator. */
        if (sta_target_ip == NULL) {
            return -2;
        }
        strncpy(out_ip, sta_target_ip, out_len - 1);
        out_ip[out_len - 1] = '\0';
        return 0;
    }

    /* SoftAP mode: stream to the connected rescuer, or broadcast on the AP
     * subnet when no specific client IP is known yet. */
    if (s_state.client_connected && s_state.client_ip[0] != '\0') {
        strncpy(out_ip, s_state.client_ip, out_len - 1);
    } else {
        strncpy(out_ip, "192.168.4.255", out_len - 1);
    }
    out_ip[out_len - 1] = '\0';
    return 0;
}
