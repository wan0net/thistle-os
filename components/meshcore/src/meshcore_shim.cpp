// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — MeshCore C shim implementation
//
// Bridges MeshCore's C++ classes to the ThistleOS HAL radio vtable.
// Any registered radio driver (SX1262, SX1276, etc.) works transparently.

#include "meshcore_shim.h"

#include <string.h>
#include <esp_log.h>
#include <esp_timer.h>
#include <esp_random.h>

// Suppress ESP32 FS.h include in SimpleMeshTables
#undef ESP32

// MeshCore headers
#include <Mesh.h>
#include <Dispatcher.h>
#include <Identity.h>
#include <Packet.h>
#include <helpers/BaseChatMesh.h>
#include <helpers/SimpleMeshTables.h>
#include <helpers/StaticPoolPacketManager.h>

#define ESP32 1

// ThistleOS HAL
extern "C" {
#include "hal/board.h"
}

static const char* TAG = "meshcore";

// FakeSerial instance for Arduino.h stub
FakeSerial Serial;

// ── HAL Radio Adapter ───────────────────────────────────────────────
//
// Bridges mesh::Radio to our hal_radio_driver_t vtable.
// MeshCore calls recvRaw()/startSendRaw() — we forward to the HAL.

class HalRadioAdapter : public mesh::Radio {
    const hal_radio_driver_t* _drv;
    uint8_t _rx_buf[256];
    int _rx_len;
    bool _send_pending;
    float _last_rssi;
    float _last_snr;

public:
    HalRadioAdapter() : _drv(nullptr), _rx_len(0), _send_pending(false),
                         _last_rssi(0), _last_snr(0) {}

    void setDriver(const hal_radio_driver_t* drv) { _drv = drv; }

    void begin() override {
        if (_drv && _drv->start_receive) {
            // Start the radio in receive mode with our callback
            _drv->start_receive(on_rx_packet, this);
        }
    }

    int recvRaw(uint8_t* bytes, int sz) override {
        if (_rx_len <= 0) return 0;
        int n = (_rx_len < sz) ? _rx_len : sz;
        memcpy(bytes, _rx_buf, n);
        _rx_len = 0;
        return n;
    }

    uint32_t getEstAirtimeFor(int len_bytes) override {
        // Rough estimate for LoRa SF7 BW125: ~30ms + 0.5ms per byte
        return 30 + (uint32_t)(len_bytes * 0.5);
    }

    float packetScore(float snr, int packet_len) override {
        return snr;
    }

    bool startSendRaw(const uint8_t* bytes, int len) override {
        if (!_drv || !_drv->send) return false;
        _send_pending = true;
        esp_err_t ret = _drv->send(bytes, len);
        _send_pending = false;
        return (ret == ESP_OK);
    }

    bool isSendComplete() override { return !_send_pending; }
    void onSendFinished() override { _send_pending = false; }
    bool isInRecvMode() const override { return !_send_pending; }
    float getLastRSSI() const override { return _last_rssi; }
    float getLastSNR() const override { return _last_snr; }

private:
    static void on_rx_packet(const uint8_t* data, size_t len, int rssi, void* ctx) {
        auto* self = static_cast<HalRadioAdapter*>(ctx);
        if (len > sizeof(self->_rx_buf)) len = sizeof(self->_rx_buf);
        memcpy(self->_rx_buf, data, len);
        self->_rx_len = (int)len;
        self->_last_rssi = (float)rssi;
    }
};

// ── Clock / RNG Adapters ────────────────────────────────────────────

class EspMillisClock : public mesh::MillisecondClock {
public:
    unsigned long getMillis() override {
        return (unsigned long)(esp_timer_get_time() / 1000ULL);
    }
};

class EspRNG : public mesh::RNG {
public:
    void random(uint8_t* dest, size_t sz) override {
        esp_fill_random(dest, sz);
    }
};

class EspRTCClock : public mesh::RTCClock {
public:
    uint32_t getCurrentTime() override {
        return (uint32_t)(esp_timer_get_time() / 1000000ULL);
    }
    void setCurrentTime(uint32_t time) override {
        // Could store offset — for now just log
        ESP_LOGI(TAG, "RTC set to %u", (unsigned)time);
    }
};

// ── ThistleOS Chat Mesh ─────────────────────────────────────────────
//
// Our concrete BaseChatMesh that dispatches events to C callbacks.

class ThistleChatMesh : public BaseChatMesh {
    meshcore_msg_cb_t _msg_cb;
    void* _msg_ud;
    meshcore_contact_cb_t _contact_cb;
    void* _contact_ud;
    meshcore_stats_t _stats;
    char _name[MESHCORE_NAME_MAX];

public:
    ThistleChatMesh(mesh::Radio& radio, mesh::MillisecondClock& ms,
                    mesh::RNG& rng, mesh::RTCClock& rtc,
                    mesh::PacketManager& mgr, mesh::MeshTables& tables)
        : BaseChatMesh(radio, ms, rng, rtc, mgr, tables),
          _msg_cb(nullptr), _msg_ud(nullptr),
          _contact_cb(nullptr), _contact_ud(nullptr)
    {
        memset(&_stats, 0, sizeof(_stats));
        memset(_name, 0, sizeof(_name));
    }

    void setName(const char* name) {
        strncpy(_name, name, MESHCORE_NAME_MAX - 1);
    }
    const char* getName() const { return _name; }
    meshcore_stats_t* stats() { return &_stats; }

    void setMessageCallback(meshcore_msg_cb_t cb, void* ud) { _msg_cb = cb; _msg_ud = ud; }
    void setContactCallback(meshcore_contact_cb_t cb, void* ud) { _contact_cb = cb; _contact_ud = ud; }

    // Convert MeshCore ContactInfo to our C struct
    static void toContact(const ContactInfo& ci, meshcore_contact_t* out) {
        memcpy(out->pub_key, ci.id.pub_key, MESHCORE_PUBKEY_SIZE);
        memset(out->name, 0, MESHCORE_NAME_MAX);
        size_t nlen = strlen(ci.name);
        if (nlen > MESHCORE_NAME_MAX - 1) nlen = MESHCORE_NAME_MAX - 1;
        memcpy(out->name, ci.name, nlen);
        out->name_len = (uint8_t)nlen;
        out->type = ci.type;
        out->last_rssi = 0; // ContactInfo doesn't track RSSI per-contact
        out->path_len = ci.out_path_len;
        out->last_seen = ci.lastmod;
        out->lat = ci.gps_lat / 1000000.0;
        out->lon = ci.gps_lon / 1000000.0;
        out->has_position = (ci.gps_lat != 0 || ci.gps_lon != 0);
    }

protected:
    // Called when a message is received
    void onMessageRecv(const ContactInfo& contact, mesh::Packet* pkt,
                       uint32_t sender_timestamp, const char* text) override {
        _stats.messages_received++;
        if (_msg_cb) {
            meshcore_contact_t c;
            toContact(contact, &c);
            _msg_cb(&c, sender_timestamp, text, _msg_ud);
        }
    }

    void onCommandDataRecv(const ContactInfo& contact, mesh::Packet* pkt,
                           uint32_t sender_timestamp, const char* text) override {
        // Route command data through the same message callback
        onMessageRecv(contact, pkt, sender_timestamp, text);
    }

    void onSignedMessageRecv(const ContactInfo& contact, mesh::Packet* pkt,
                             uint32_t sender_timestamp, const uint8_t* sender_prefix,
                             const char* text) override {
        onMessageRecv(contact, pkt, sender_timestamp, text);
    }

    void onDiscoveredContact(ContactInfo& contact, bool is_new,
                             uint8_t path_len, const uint8_t* path) override {
        if (is_new) _stats.contacts_discovered++;
        if (_contact_cb) {
            meshcore_contact_t c;
            toContact(contact, &c);
            _contact_cb(&c, is_new, _contact_ud);
        }
    }

    ContactInfo* processAck(const uint8_t* data) override {
        return checkConnectionsAck(data);
    }

    void onContactPathUpdated(const ContactInfo& contact) override {
        // Could notify UI that a route changed
    }

    uint32_t calcFloodTimeoutMillisFor(uint32_t pkt_airtime_millis) const override {
        return pkt_airtime_millis * 5 + 2000;
    }

    uint32_t calcDirectTimeoutMillisFor(uint32_t pkt_airtime_millis, uint8_t path_len) const override {
        return pkt_airtime_millis * (path_len + 2) * 3 + 2000;
    }

    void onSendTimeout() override {
        ESP_LOGW(TAG, "Message send timed out");
    }

    void onChannelMessageRecv(const mesh::GroupChannel& channel, mesh::Packet* pkt,
                              uint32_t timestamp, const char* text) override {
        // Group channel messages — route as regular messages for now
        _stats.messages_received++;
        if (_msg_cb) {
            meshcore_contact_t c;
            memset(&c, 0, sizeof(c));
            strncpy(c.name, "[group]", MESHCORE_NAME_MAX - 1);
            c.name_len = 7;
            _msg_cb(&c, timestamp, text, _msg_ud);
        }
    }

    uint8_t onContactRequest(const ContactInfo& contact, uint32_t sender_timestamp,
                             const uint8_t* data, uint8_t len, uint8_t* reply) override {
        return 0; // no custom request handling
    }

    void onContactResponse(const ContactInfo& contact, const uint8_t* data, uint8_t len) override {
        // no-op
    }
};

// ── Global State ────────────────────────────────────────────────────

static HalRadioAdapter s_radio;
static EspMillisClock s_clock;
static EspRNG s_rng;
static EspRTCClock s_rtc;

#define PACKET_POOL_SIZE 16
static StaticPoolPacketManager s_pkt_mgr(PACKET_POOL_SIZE);
static SimpleMeshTables s_tables;

static ThistleChatMesh* s_mesh = nullptr;
static bool s_initialized = false;

// ── C API Implementation ────────────────────────────────────────────

extern "C" {

esp_err_t meshcore_init(const char* node_name, uint8_t node_type) {
    if (s_initialized) return ESP_OK;

    // Get the HAL radio driver
    const hal_registry_t* reg = hal_get_registry();
    if (!reg || !reg->radio) {
        ESP_LOGE(TAG, "No radio driver registered");
        return ESP_ERR_NOT_FOUND;
    }

    s_radio.setDriver(reg->radio);

    // Create the mesh instance
    s_mesh = new ThistleChatMesh(s_radio, s_clock, s_rng, s_rtc, s_pkt_mgr, s_tables);
    if (!s_mesh) return ESP_ERR_NO_MEM;

    s_mesh->setName(node_name);

    // Generate a random identity
    s_mesh->self_id = mesh::LocalIdentity(&s_rng);

    // Start the radio
    s_radio.begin();

    s_initialized = true;
    ESP_LOGI(TAG, "MeshCore initialized: %s (type=%d)", node_name, node_type);
    return ESP_OK;
}

esp_err_t meshcore_deinit(void) {
    if (!s_initialized) return ESP_OK;
    delete s_mesh;
    s_mesh = nullptr;
    s_initialized = false;
    return ESP_OK;
}

esp_err_t meshcore_loop(void) {
    if (!s_mesh) return ESP_ERR_INVALID_STATE;
    s_mesh->loop();
    return ESP_OK;
}

int meshcore_send_message(const uint8_t* dest_pub_key, const char* text) {
    if (!s_mesh || !dest_pub_key || !text) return MESHCORE_SEND_FAILED;

    // Find the contact by public key
    ContactsIterator iter;
    ContactInfo ci;
    int idx = 0;
    bool found = false;

    // Search contacts
    ContactsIterator it;
    while (it.hasNext(s_mesh, ci)) {
        if (memcmp(ci.id.pub_key, dest_pub_key, MESHCORE_PUBKEY_SIZE) == 0) {
            found = true;
            break;
        }
    }

    if (!found) return MESHCORE_SEND_FAILED;

    uint32_t timestamp = s_rtc.getCurrentTime();
    uint32_t expected_ack = 0, est_timeout = 0;
    int result = s_mesh->sendMessage(ci, timestamp, 0, text, expected_ack, est_timeout);

    s_mesh->stats()->messages_sent++;
    s_mesh->stats()->packets_sent++;

    return result;
}

esp_err_t meshcore_send_advert(void) {
    if (!s_mesh) return ESP_ERR_INVALID_STATE;
    mesh::Packet* pkt = s_mesh->createSelfAdvert(s_mesh->getName());
    if (!pkt) return ESP_ERR_NO_MEM;
    s_mesh->sendPacket(pkt, 1);
    s_mesh->stats()->packets_sent++;
    return ESP_OK;
}

esp_err_t meshcore_send_advert_with_position(double lat, double lon) {
    if (!s_mesh) return ESP_ERR_INVALID_STATE;
    mesh::Packet* pkt = s_mesh->createSelfAdvert(s_mesh->getName(), lat, lon);
    if (!pkt) return ESP_ERR_NO_MEM;
    s_mesh->sendPacket(pkt, 1);
    s_mesh->stats()->packets_sent++;
    return ESP_OK;
}

int meshcore_get_contact_count(void) {
    if (!s_mesh) return 0;
    int count = 0;
    ContactsIterator it;
    ContactInfo ci;
    while (it.hasNext(s_mesh, ci)) count++;
    return count;
}

esp_err_t meshcore_get_contact(int index, meshcore_contact_t* out) {
    if (!s_mesh || !out) return ESP_ERR_INVALID_ARG;
    ContactsIterator it;
    ContactInfo ci;
    int i = 0;
    while (it.hasNext(s_mesh, ci)) {
        if (i == index) {
            ThistleChatMesh::toContact(ci, out);
            return ESP_OK;
        }
        i++;
    }
    return ESP_ERR_NOT_FOUND;
}

int meshcore_find_contact(const uint8_t* pub_key) {
    if (!s_mesh || !pub_key) return -1;
    ContactsIterator it;
    ContactInfo ci;
    int i = 0;
    while (it.hasNext(s_mesh, ci)) {
        if (memcmp(ci.id.pub_key, pub_key, MESHCORE_PUBKEY_SIZE) == 0) return i;
        i++;
    }
    return -1;
}

void meshcore_set_message_callback(meshcore_msg_cb_t cb, void* user_data) {
    if (s_mesh) s_mesh->setMessageCallback(cb, user_data);
}

void meshcore_set_contact_callback(meshcore_contact_cb_t cb, void* user_data) {
    if (s_mesh) s_mesh->setContactCallback(cb, user_data);
}

esp_err_t meshcore_get_self_pub_key(uint8_t* out_key) {
    if (!s_mesh || !out_key) return ESP_ERR_INVALID_ARG;
    memcpy(out_key, s_mesh->self_id.pub_key, MESHCORE_PUBKEY_SIZE);
    return ESP_OK;
}

const char* meshcore_get_self_name(void) {
    if (!s_mesh) return "";
    return s_mesh->getName();
}

esp_err_t meshcore_get_stats(meshcore_stats_t* out) {
    if (!s_mesh || !out) return ESP_ERR_INVALID_ARG;
    memcpy(out, s_mesh->stats(), sizeof(meshcore_stats_t));
    return ESP_OK;
}

} // extern "C"
