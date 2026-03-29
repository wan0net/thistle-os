/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Vault app UI
 *
 * Three-screen password vault:
 *   Lock screen  — master-password entry, PBKDF2 key derivation
 *   Entry list   — scrollable list of stored credentials
 *   Entry detail — name / username / password / notes editor
 *
 * Encryption: AES-256-CBC + PBKDF2-HMAC-SHA256 + HMAC-SHA256 integrity.
 * File layout: [16-byte salt][16-byte IV][encrypted JSON][32-byte HMAC]
 *
 * Works on all platforms (ESP32, simulator, WASM) via the kernel crypto module.
 */
#include "vault/vault_app.h"

#include "hal/sdcard_path.h"
#include "ui/theme.h"
#include "ui/toast.h"
#include "thistle/app_manager.h"

#include "lvgl.h"
#include "esp_log.h"

#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <sys/stat.h>

/* Kernel crypto — works on all platforms (ESP32, simulator, WASM) */
extern int thistle_crypto_sha256(const unsigned char *data, unsigned int len, unsigned char *hash_out);
extern int thistle_crypto_hmac_sha256(const unsigned char *key, unsigned int key_len, const unsigned char *data, unsigned int data_len, unsigned char *mac_out);
extern int thistle_crypto_hmac_verify(const unsigned char *key, unsigned int key_len, const unsigned char *data, unsigned int data_len, const unsigned char *expected_mac);
extern int thistle_crypto_aes256_cbc_encrypt(const unsigned char *key, const unsigned char *iv, const unsigned char *plaintext, unsigned int len, unsigned char *ciphertext_out);
extern int thistle_crypto_aes256_cbc_decrypt(const unsigned char *key, const unsigned char *iv, const unsigned char *ciphertext, unsigned int len, unsigned char *plaintext_out);
extern int thistle_crypto_pbkdf2_sha256(const char *password, const unsigned char *salt, unsigned int salt_len, unsigned int iterations, unsigned char *key_out, unsigned int key_len);
extern int thistle_crypto_random(unsigned char *buf, unsigned int len);

static const char *TAG = "vault_ui";

/* ------------------------------------------------------------------ */
/* Constants                                                            */
/* ------------------------------------------------------------------ */

static int s_app_w = 240;
static int s_app_h = 296;
#define HEADER_H      30
#define ITEM_H        32
#define FIELD_MAX     64
#define MAX_ENTRIES   32

#define VAULT_PATH   THISTLE_SDCARD "/config/vault.enc"
#define VAULT_CONFIG_DIR  THISTLE_SDCARD "/config"

/* File format offsets */
#define SALT_LEN     16
#define IV_LEN       16
#define HMAC_LEN     32
#define HEADER_LEN   (SALT_LEN + IV_LEN)   /* 32 bytes before ciphertext */

/* PBKDF2 iterations — deliberately slow */
#define PBKDF2_ITER  10000

/* ------------------------------------------------------------------ */
/* Data types                                                           */
/* ------------------------------------------------------------------ */

typedef struct {
    char name[FIELD_MAX];
    char username[FIELD_MAX];
    char password[FIELD_MAX];
    char notes[FIELD_MAX];
} vault_entry_t;

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t *root;

    /* Screen panels */
    lv_obj_t *lock_screen;
    lv_obj_t *list_screen;
    lv_obj_t *detail_screen;

    /* Lock screen widgets */
    lv_obj_t *master_pw_ta;
    lv_obj_t *unlock_status_lbl;

    /* List screen widgets */
    lv_obj_t *list_container;

    /* Detail screen widgets */
    lv_obj_t *det_name_ta;
    lv_obj_t *det_user_ta;
    lv_obj_t *det_pass_ta;
    lv_obj_t *det_notes_ta;
    lv_obj_t *det_pass_toggle_btn;
    bool      det_pass_visible;

    /* Vault data */
    vault_entry_t entries[MAX_ENTRIES];
    int           entry_count;

    /* Crypto state — zeroed on lock */
    uint8_t  derived_key[32];
    uint8_t  salt[SALT_LEN];
    bool     unlocked;

    /* Detail screen context */
    int  selected_idx;   /* -1 = new entry */
} s_vault;

/* ------------------------------------------------------------------ */
/* Forward declarations                                                 */
/* ------------------------------------------------------------------ */

static void switch_to_lock(void);
static void switch_to_list(void);
static void switch_to_detail(int idx);
static void populate_list(void);
static esp_err_t vault_load(const char *master_pw);
static esp_err_t vault_save(void);

/* ------------------------------------------------------------------ */
/* Crypto helpers — unified, backed by the kernel crypto module        */
/* ------------------------------------------------------------------ */

static esp_err_t derive_key(const char *password,
                             const uint8_t salt[SALT_LEN],
                             uint8_t key[32])
{
    return (thistle_crypto_pbkdf2_sha256(password, salt, SALT_LEN, PBKDF2_ITER, key, 32) == 0)
           ? ESP_OK : ESP_FAIL;
}

static esp_err_t vault_encrypt(const uint8_t *plaintext, size_t len,
                                const uint8_t key[32],
                                const uint8_t iv[IV_LEN],
                                uint8_t *ciphertext)
{
    return (thistle_crypto_aes256_cbc_encrypt(key, iv, plaintext, len, ciphertext) == 0)
           ? ESP_OK : ESP_FAIL;
}

static esp_err_t vault_decrypt(const uint8_t *ciphertext, size_t len,
                                const uint8_t key[32],
                                const uint8_t iv[IV_LEN],
                                uint8_t *plaintext)
{
    return (thistle_crypto_aes256_cbc_decrypt(key, iv, ciphertext, len, plaintext) == 0)
           ? ESP_OK : ESP_FAIL;
}

static void compute_hmac(const uint8_t *data, size_t data_len,
                          const uint8_t key[32],
                          uint8_t hmac_out[HMAC_LEN])
{
    thistle_crypto_hmac_sha256(key, 32, data, data_len, hmac_out);
}

static void fill_random(uint8_t *buf, size_t len)
{
    thistle_crypto_random(buf, len);
}

/* ------------------------------------------------------------------ */
/* Simple JSON serialiser / parser for vault entries                   */
/*                                                                      */
/* Format: [{"name":"...","user":"...","pass":"...","notes":"..."},…] */
/* Avoids pulling in a full JSON library.                              */
/* ------------------------------------------------------------------ */

/* Escape a string for JSON (replaces " and \ ) */
static size_t json_escape(const char *src, char *dst, size_t dst_max)
{
    size_t out = 0;
    for (size_t i = 0; src[i] && out + 3 < dst_max; i++) {
        if (src[i] == '"' || src[i] == '\\') {
            dst[out++] = '\\';
        }
        dst[out++] = src[i];
    }
    dst[out] = '\0';
    return out;
}

/* Unescape a JSON string value — writes into dst, returns chars consumed
 * from src (stopping at closing unescaped '"'). */
static size_t json_unescape(const char *src, char *dst, size_t dst_max)
{
    size_t in = 0, out = 0;
    while (src[in] && out + 1 < dst_max) {
        if (src[in] == '"') { in++; break; } /* end of value */
        if (src[in] == '\\' && src[in + 1]) {
            in++;
            dst[out++] = src[in++];
        } else {
            dst[out++] = src[in++];
        }
    }
    dst[out] = '\0';
    return in;
}

/* Serialise all entries into a malloc'd JSON buffer.
 * Returns NULL on OOM.  Caller must free(). */
static char *entries_to_json(size_t *out_len)
{
    /* Upper bound: 4 fields * (FIELD_MAX*2 + overhead) per entry */
    size_t cap = (size_t)s_vault.entry_count * (FIELD_MAX * 8 + 64) + 16;
    char *buf = (char *)malloc(cap);
    if (!buf) return NULL;

    size_t pos = 0;
    if (pos + 1 < cap) buf[pos++] = '[';

    for (int i = 0; i < s_vault.entry_count; i++) {
        const vault_entry_t *e = &s_vault.entries[i];
        char esc[FIELD_MAX * 2 + 2];

        if (i > 0) { if (pos + 1 < cap) buf[pos++] = ','; }
        if (pos + 1 < cap) buf[pos++] = '{';

#define APPEND_FIELD(key, val) do { \
    json_escape((val), esc, sizeof(esc)); \
    pos += (size_t)snprintf(buf + pos, cap - pos, \
                            "\"" key "\":\"%s\"", esc); \
} while (0)

        APPEND_FIELD("name",  e->name);     if (pos + 1 < cap) buf[pos++] = ',';
        APPEND_FIELD("user",  e->username); if (pos + 1 < cap) buf[pos++] = ',';
        APPEND_FIELD("pass",  e->password); if (pos + 1 < cap) buf[pos++] = ',';
        APPEND_FIELD("notes", e->notes);

#undef APPEND_FIELD

        if (pos + 1 < cap) buf[pos++] = '}';
    }

    if (pos + 1 < cap) buf[pos++] = ']';
    buf[pos]   = '\0';
    *out_len = pos;
    return buf;
}

/* Parse one string value that starts after the opening '"'.
 * src_pos advances past the closing '"'. */
static void parse_field(const char *json, size_t *src_pos,
                         char *dst, size_t dst_max)
{
    *src_pos += json_unescape(json + *src_pos, dst, dst_max);
}

/* Skip past the key + colon + opening quote.  Returns false if not found. */
static bool seek_key(const char *json, size_t *pos, const char *key)
{
    /* Find: "key":"  */
    char needle[72];
    snprintf(needle, sizeof(needle), "\"%s\":\"", key);
    const char *found = strstr(json + *pos, needle);
    if (!found) return false;
    *pos = (size_t)(found - json) + strlen(needle);
    return true;
}

/* Deserialise JSON into s_vault.entries.  Returns entry count or -1. */
static int json_to_entries(const char *json)
{
    int count = 0;
    size_t pos = 0;
    size_t len = strlen(json);

    while (pos < len && count < MAX_ENTRIES) {
        /* Find start of next object */
        const char *obj = strchr(json + pos, '{');
        if (!obj) break;
        pos = (size_t)(obj - json) + 1;

        vault_entry_t *e = &s_vault.entries[count];
        memset(e, 0, sizeof(*e));

        /* Each seek operates from the current pos onward */
        size_t p = pos;
        if (!seek_key(json, &p, "name"))  goto next;
        parse_field(json, &p, e->name, FIELD_MAX);

        p = pos;
        if (!seek_key(json, &p, "user"))  goto next;
        parse_field(json, &p, e->username, FIELD_MAX);

        p = pos;
        if (!seek_key(json, &p, "pass"))  goto next;
        parse_field(json, &p, e->password, FIELD_MAX);

        p = pos;
        if (!seek_key(json, &p, "notes")) goto next;
        parse_field(json, &p, e->notes, FIELD_MAX);

        count++;
next:
        /* Advance past this object */
        {
            const char *end = strchr(json + pos, '}');
            if (!end) break;
            pos = (size_t)(end - json) + 1;
        }
    }

    return count;
}

/* ------------------------------------------------------------------ */
/* Pad / unpad for AES-CBC (PKCS#7)                                    */
/* ------------------------------------------------------------------ */

/* Returns the padded length (multiple of 16). */
static size_t pkcs7_pad(const uint8_t *in, size_t in_len,
                         uint8_t **out)
{
    size_t padded = ((in_len / 16) + 1) * 16;
    uint8_t pad_byte = (uint8_t)(padded - in_len);
    *out = (uint8_t *)malloc(padded);
    if (!*out) return 0;
    memcpy(*out, in, in_len);
    memset(*out + in_len, pad_byte, padded - in_len);
    return padded;
}

/* Returns unpadded length, or 0 on invalid padding. */
static size_t pkcs7_unpad(uint8_t *buf, size_t padded_len)
{
    if (padded_len == 0 || padded_len % 16 != 0) return 0;
    uint8_t pad = buf[padded_len - 1];
    if (pad == 0 || pad > 16) return 0;
    for (size_t i = padded_len - pad; i < padded_len; i++) {
        if (buf[i] != pad) return 0;
    }
    return padded_len - pad;
}

/* ------------------------------------------------------------------ */
/* File I/O                                                             */
/* ------------------------------------------------------------------ */

static void ensure_config_dir(void)
{
    struct stat st;
    if (stat(VAULT_CONFIG_DIR, &st) != 0) {
        if (mkdir(VAULT_CONFIG_DIR, 0755) != 0) {
            ESP_LOGW(TAG, "mkdir %s failed", VAULT_CONFIG_DIR);
        }
    }
}

static bool vault_file_exists(void)
{
    struct stat st;
    return (stat(VAULT_PATH, &st) == 0);
}

/*
 * vault_load — open vault file, verify HMAC, decrypt, parse JSON.
 *
 * If the vault file does not exist this is "first use": the master
 * password is accepted as-is, a random salt is generated, and an
 * empty vault is created.
 *
 * Returns ESP_OK on success, ESP_ERR_INVALID_ARG on wrong password
 * (HMAC mismatch), other errors for I/O / crypto failure.
 */
static esp_err_t vault_load(const char *master_pw)
{
    ensure_config_dir();

    if (!vault_file_exists()) {
        /* First use: create empty vault */
        ESP_LOGI(TAG, "First use — creating new vault");
        fill_random(s_vault.salt, SALT_LEN);
        s_vault.entry_count = 0;

        esp_err_t rc = derive_key(master_pw, s_vault.salt, s_vault.derived_key);
        if (rc != ESP_OK) {
            ESP_LOGE(TAG, "Key derivation failed on first use");
            return rc;
        }

        /* Save the (empty) vault immediately */
        esp_err_t save_rc = vault_save();
        if (save_rc != ESP_OK) {
            ESP_LOGW(TAG, "Could not write initial vault file: %d", save_rc);
            /* Not fatal — carry on with in-memory empty vault */
        }
        return ESP_OK;
    }

    FILE *f = fopen(VAULT_PATH, "rb");
    if (!f) return ESP_ERR_NOT_FOUND;

    fseek(f, 0, SEEK_END);
    long fsz = ftell(f);
    fseek(f, 0, SEEK_SET);

    /* Minimum: salt + IV + 16 bytes ciphertext + HMAC */
    if (fsz < (long)(HEADER_LEN + 16 + HMAC_LEN)) {
        fclose(f);
        ESP_LOGE(TAG, "Vault file too small (%ld bytes)", fsz);
        return ESP_ERR_INVALID_SIZE;
    }

    uint8_t *filebuf = (uint8_t *)malloc((size_t)fsz);
    if (!filebuf) { fclose(f); return ESP_ERR_NO_MEM; }
    size_t nread_vault = fread(filebuf, 1, (size_t)fsz, f);
    fclose(f);
    if (nread_vault != (size_t)fsz) { free(filebuf); return ESP_ERR_INVALID_SIZE; }

    /* Split file */
    uint8_t *file_salt = filebuf;                            /* [0..15]  */
    uint8_t *file_iv   = filebuf + SALT_LEN;                /* [16..31] */
    uint8_t *ciphertext = filebuf + HEADER_LEN;             /* [32..fsz-HMAC_LEN-1] */
    size_t   cipher_len = (size_t)fsz - HEADER_LEN - HMAC_LEN;
    uint8_t *file_hmac  = filebuf + fsz - HMAC_LEN;

    /* Derive key from candidate password */
    uint8_t candidate_key[32];
    esp_err_t rc = derive_key(master_pw, file_salt, candidate_key);
    if (rc != ESP_OK) {
        free(filebuf);
        return rc;
    }

    /* Verify HMAC over (salt || IV || ciphertext) */
    if (thistle_crypto_hmac_verify(candidate_key, 32,
                                    filebuf, (size_t)fsz - HMAC_LEN,
                                    file_hmac) != 0) {
        free(filebuf);
        memset(candidate_key, 0, 32);
        ESP_LOGW(TAG, "HMAC mismatch — wrong password or corrupted vault");
        return ESP_ERR_INVALID_ARG;  /* wrong password */
    }

    /* Decrypt */
    uint8_t *plaintext = (uint8_t *)malloc(cipher_len + 1);
    if (!plaintext) { free(filebuf); memset(candidate_key, 0, 32); return ESP_ERR_NO_MEM; }

    rc = vault_decrypt(ciphertext, cipher_len, candidate_key, file_iv, plaintext);
    if (rc != ESP_OK) {
        free(plaintext);
        free(filebuf);
        memset(candidate_key, 0, 32);
        return rc;
    }

    /* Unpad */
    size_t plain_len = pkcs7_unpad(plaintext, cipher_len);
    if (plain_len == 0) {
        free(plaintext);
        free(filebuf);
        memset(candidate_key, 0, 32);
        ESP_LOGE(TAG, "Invalid PKCS7 padding");
        return ESP_ERR_INVALID_ARG;
    }
    plaintext[plain_len] = '\0';

    /* Parse JSON */
    int cnt = json_to_entries((char *)plaintext);
    free(plaintext);

    /* Accept key and salt only after successful authentication */
    memcpy(s_vault.salt, file_salt, SALT_LEN);
    memcpy(s_vault.derived_key, candidate_key, 32);
    memset(candidate_key, 0, 32);
    free(filebuf);

    s_vault.entry_count = (cnt >= 0) ? cnt : 0;
    return ESP_OK;
}

/*
 * vault_save — encrypt the current entry list and write to SD card.
 */
static esp_err_t vault_save(void)
{
    ensure_config_dir();

    /* Serialise entries to JSON */
    size_t json_len = 0;
    char *json = entries_to_json(&json_len);
    if (!json) return ESP_ERR_NO_MEM;

    /* PKCS7-pad the JSON */
    uint8_t *padded = NULL;
    size_t padded_len = pkcs7_pad((const uint8_t *)json, json_len, &padded);
    free(json);
    if (padded_len == 0) return ESP_ERR_NO_MEM;

    /* Generate fresh IV for this save */
    uint8_t iv[IV_LEN];
    fill_random(iv, IV_LEN);

    /* Encrypt */
    uint8_t *ciphertext = (uint8_t *)malloc(padded_len);
    if (!ciphertext) { free(padded); return ESP_ERR_NO_MEM; }

    esp_err_t rc = vault_encrypt(padded, padded_len,
                                  s_vault.derived_key, iv,
                                  ciphertext);
    free(padded);
    if (rc != ESP_OK) { free(ciphertext); return rc; }

    /* Build file: salt || IV || ciphertext */
    size_t body_len = SALT_LEN + IV_LEN + padded_len;
    uint8_t *filebuf = (uint8_t *)malloc(body_len + HMAC_LEN);
    if (!filebuf) { free(ciphertext); return ESP_ERR_NO_MEM; }

    memcpy(filebuf,                  s_vault.salt, SALT_LEN);
    memcpy(filebuf + SALT_LEN,       iv,           IV_LEN);
    memcpy(filebuf + SALT_LEN + IV_LEN, ciphertext, padded_len);
    free(ciphertext);

    /* Compute HMAC over (salt || IV || ciphertext) */
    compute_hmac(filebuf, body_len, s_vault.derived_key,
                 filebuf + body_len);

    FILE *f = fopen(VAULT_PATH, "wb");
    if (!f) { free(filebuf); return ESP_ERR_NOT_FOUND; }
    fwrite(filebuf, 1, body_len + HMAC_LEN, f);
    fclose(f);
    free(filebuf);

    ESP_LOGI(TAG, "Vault saved (%d entries)", s_vault.entry_count);
    return ESP_OK;
}

/* ------------------------------------------------------------------ */
/* Public: lock — zero key, go to lock screen                         */
/* ------------------------------------------------------------------ */

void vault_ui_lock(void)
{
    if (!s_vault.unlocked) return;

    /* Zero the derived key from RAM */
    memset(s_vault.derived_key, 0, sizeof(s_vault.derived_key));
    s_vault.unlocked = false;
    s_vault.entry_count = 0;

    ESP_LOGI(TAG, "Vault locked, key zeroed");

    if (s_vault.root && !lv_obj_has_flag(s_vault.root, LV_OBJ_FLAG_HIDDEN)) {
        switch_to_lock();
    }
}

/* ------------------------------------------------------------------ */
/* Screen switching                                                     */
/* ------------------------------------------------------------------ */

static void switch_to_lock(void)
{
    lv_obj_clear_flag(s_vault.lock_screen,   LV_OBJ_FLAG_HIDDEN);
    lv_obj_add_flag(s_vault.list_screen,     LV_OBJ_FLAG_HIDDEN);
    lv_obj_add_flag(s_vault.detail_screen,   LV_OBJ_FLAG_HIDDEN);

    /* Clear password field */
    if (s_vault.master_pw_ta) {
        lv_textarea_set_text(s_vault.master_pw_ta, "");
    }
    if (s_vault.unlock_status_lbl) {
        lv_label_set_text(s_vault.unlock_status_lbl, "");
    }

    /* Focus password field */
    lv_group_t *grp = lv_group_get_default();
    if (grp && s_vault.master_pw_ta) {
        lv_group_add_obj(grp, s_vault.master_pw_ta);
        lv_group_focus_obj(s_vault.master_pw_ta);
    }
}

static void switch_to_list(void)
{
    lv_obj_add_flag(s_vault.lock_screen,     LV_OBJ_FLAG_HIDDEN);
    lv_obj_clear_flag(s_vault.list_screen,   LV_OBJ_FLAG_HIDDEN);
    lv_obj_add_flag(s_vault.detail_screen,   LV_OBJ_FLAG_HIDDEN);

    populate_list();
}

static void switch_to_detail(int idx)
{
    s_vault.selected_idx = idx;
    s_vault.det_pass_visible = false;

    lv_obj_add_flag(s_vault.lock_screen,     LV_OBJ_FLAG_HIDDEN);
    lv_obj_add_flag(s_vault.list_screen,     LV_OBJ_FLAG_HIDDEN);
    lv_obj_clear_flag(s_vault.detail_screen, LV_OBJ_FLAG_HIDDEN);

    /* Populate fields */
    if (idx >= 0 && idx < s_vault.entry_count) {
        const vault_entry_t *e = &s_vault.entries[idx];
        lv_textarea_set_text(s_vault.det_name_ta,  e->name);
        lv_textarea_set_text(s_vault.det_user_ta,  e->username);
        lv_textarea_set_text(s_vault.det_pass_ta,  e->password);
        lv_textarea_set_text(s_vault.det_notes_ta, e->notes);
    } else {
        /* New entry */
        lv_textarea_set_text(s_vault.det_name_ta,  "");
        lv_textarea_set_text(s_vault.det_user_ta,  "");
        lv_textarea_set_text(s_vault.det_pass_ta,  "");
        lv_textarea_set_text(s_vault.det_notes_ta, "");
    }

    /* Password hidden by default */
    lv_textarea_set_password_mode(s_vault.det_pass_ta, true);

    /* Update toggle button label */
    lv_obj_t *lbl = lv_obj_get_child(s_vault.det_pass_toggle_btn, 0);
    if (lbl) lv_label_set_text(lbl, "Show");

    /* Focus name field */
    lv_group_t *grp = lv_group_get_default();
    if (grp) {
        lv_group_add_obj(grp, s_vault.det_name_ta);
        lv_group_focus_obj(s_vault.det_name_ta);
    }
}

/* ------------------------------------------------------------------ */
/* List population                                                      */
/* ------------------------------------------------------------------ */

static void entry_row_clicked_cb(lv_event_t *e);

static void populate_list(void)
{
    lv_obj_clean(s_vault.list_container);
    lv_obj_scroll_to_y(s_vault.list_container, 0, LV_ANIM_OFF);

    const theme_colors_t *clr = theme_get_colors();

    if (s_vault.entry_count == 0) {
        lv_obj_t *lbl = lv_label_create(s_vault.list_container);
        lv_label_set_text(lbl, "(vault empty)");
        lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
        lv_obj_set_width(lbl, LV_PCT(100));
        lv_obj_set_style_pad_top(lbl, 20, LV_PART_MAIN);
        return;
    }

    for (int i = 0; i < s_vault.entry_count; i++) {
        const vault_entry_t *e = &s_vault.entries[i];

        lv_obj_t *row = lv_obj_create(s_vault.list_container);
        lv_obj_set_size(row, LV_PCT(100), ITEM_H);
        lv_obj_set_style_bg_color(row, clr->surface, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_left(row, 8, LV_PART_MAIN);
        lv_obj_set_style_pad_right(row, 8, LV_PART_MAIN);
        lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
        lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
        lv_obj_set_style_border_color(row, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);
        lv_obj_set_style_bg_color(row, clr->primary, LV_STATE_PRESSED);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);
        lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
        lv_obj_set_flex_align(row,
                              LV_FLEX_ALIGN_START,
                              LV_FLEX_ALIGN_CENTER,
                              LV_FLEX_ALIGN_CENTER);
        lv_obj_set_style_pad_column(row, 8, LV_PART_MAIN);
        lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);
        lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);

        /* Store index as user_data */
        lv_obj_set_user_data(row, (void *)(intptr_t)i);
        lv_obj_add_event_cb(row, entry_row_clicked_cb, LV_EVENT_CLICKED, NULL);

        /* Entry name — left, grows */
        lv_obj_t *name_lbl = lv_label_create(row);
        lv_label_set_text(name_lbl, e->name[0] ? e->name : "(unnamed)");
        lv_label_set_long_mode(name_lbl, LV_LABEL_LONG_CLIP);
        lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(name_lbl, clr->text, LV_PART_MAIN);
        lv_obj_set_style_text_color(name_lbl, clr->bg, LV_STATE_PRESSED);
        lv_obj_set_flex_grow(name_lbl, 1);

        /* Username — right, secondary colour */
        lv_obj_t *user_lbl = lv_label_create(row);
        const char *user_display = e->username[0] ? e->username : "—";
        lv_label_set_text(user_lbl, user_display);
        lv_label_set_long_mode(user_lbl, LV_LABEL_LONG_CLIP);
        lv_obj_set_style_text_font(user_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(user_lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_color(user_lbl, clr->bg, LV_STATE_PRESSED);
        lv_obj_set_width(user_lbl, 110);
    }
}

/* ------------------------------------------------------------------ */
/* Event callbacks                                                      */
/* ------------------------------------------------------------------ */

static void entry_row_clicked_cb(lv_event_t *e)
{
    lv_obj_t *row = lv_event_get_target(e);
    int idx = (int)(intptr_t)lv_obj_get_user_data(row);
    switch_to_detail(idx);
}

static void unlock_btn_cb(lv_event_t *e)
{
    (void)e;
    const char *pw = lv_textarea_get_text(s_vault.master_pw_ta);
    if (!pw || pw[0] == '\0') {
        lv_label_set_text(s_vault.unlock_status_lbl, "Enter a password");
        return;
    }

    lv_label_set_text(s_vault.unlock_status_lbl, "Unlocking...");
    lv_refr_now(NULL);

    esp_err_t rc = vault_load(pw);
    if (rc == ESP_OK) {
        s_vault.unlocked = true;
        switch_to_list();
    } else if (rc == ESP_ERR_INVALID_ARG) {
        lv_label_set_text(s_vault.unlock_status_lbl, "Wrong password");
        lv_textarea_set_text(s_vault.master_pw_ta, "");
    } else {
        lv_label_set_text(s_vault.unlock_status_lbl, "Error reading vault");
    }
}

static void lock_btn_cb(lv_event_t *e)
{
    (void)e;
    vault_ui_lock();
}

static void new_entry_btn_cb(lv_event_t *e)
{
    (void)e;
    if (s_vault.entry_count >= MAX_ENTRIES) {
        toast_warn("Vault full (32 entries max)");
        return;
    }
    switch_to_detail(-1);
}

static void back_from_detail_cb(lv_event_t *e)
{
    (void)e;
    switch_to_list();
}

static void save_entry_btn_cb(lv_event_t *e)
{
    (void)e;

    int idx = s_vault.selected_idx;
    bool is_new = (idx < 0);

    if (is_new) {
        if (s_vault.entry_count >= MAX_ENTRIES) {
            toast_warn("Vault full");
            return;
        }
        idx = s_vault.entry_count;
    }

    vault_entry_t *entry = &s_vault.entries[idx];

    /* Copy field values (clamped to FIELD_MAX-1) */
    const char *name  = lv_textarea_get_text(s_vault.det_name_ta);
    const char *user  = lv_textarea_get_text(s_vault.det_user_ta);
    const char *pass  = lv_textarea_get_text(s_vault.det_pass_ta);
    const char *notes = lv_textarea_get_text(s_vault.det_notes_ta);

    strncpy(entry->name,     name  ? name  : "", FIELD_MAX - 1);
    strncpy(entry->username, user  ? user  : "", FIELD_MAX - 1);
    strncpy(entry->password, pass  ? pass  : "", FIELD_MAX - 1);
    strncpy(entry->notes,    notes ? notes : "", FIELD_MAX - 1);
    entry->name[FIELD_MAX - 1]     = '\0';
    entry->username[FIELD_MAX - 1] = '\0';
    entry->password[FIELD_MAX - 1] = '\0';
    entry->notes[FIELD_MAX - 1]    = '\0';

    if (is_new) {
        s_vault.entry_count++;
    }

    esp_err_t rc = vault_save();
    if (rc == ESP_OK) {
        toast_show("Saved", TOAST_SUCCESS, 1500);
        switch_to_list();
    } else {
        toast_warn("Save failed");
    }
}

static void delete_entry_btn_cb(lv_event_t *e)
{
    (void)e;
    int idx = s_vault.selected_idx;
    if (idx < 0 || idx >= s_vault.entry_count) {
        switch_to_list();
        return;
    }

    /* Shift entries left */
    for (int i = idx; i < s_vault.entry_count - 1; i++) {
        s_vault.entries[i] = s_vault.entries[i + 1];
    }
    memset(&s_vault.entries[s_vault.entry_count - 1], 0, sizeof(vault_entry_t));
    s_vault.entry_count--;

    esp_err_t rc = vault_save();
    if (rc == ESP_OK) {
        toast_show("Deleted", TOAST_SUCCESS, 1500);
    } else {
        toast_warn("Delete failed");
    }
    switch_to_list();
}

static void toggle_pass_cb(lv_event_t *e)
{
    (void)e;
    s_vault.det_pass_visible = !s_vault.det_pass_visible;
    lv_textarea_set_password_mode(s_vault.det_pass_ta,
                                  !s_vault.det_pass_visible);
    lv_obj_t *lbl = lv_obj_get_child(s_vault.det_pass_toggle_btn, 0);
    if (lbl) {
        lv_label_set_text(lbl, s_vault.det_pass_visible ? "Hide" : "Show");
    }
}

static void lock_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ENTER) {
        unlock_btn_cb(NULL);
    } else if (key == LV_KEY_ESC || key == 'q' || key == 'Q') {
        app_manager_launch("com.thistle.launcher");
    }
}

static void list_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ESC || key == 'q' || key == 'Q') {
        vault_ui_lock();
    }
}

static void detail_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ESC) {
        switch_to_list();
    }
}

/* ------------------------------------------------------------------ */
/* Helper: create a labelled form row                                   */
/* ------------------------------------------------------------------ */

static lv_obj_t *make_form_row(lv_obj_t *parent, const char *label_text,
                                int y, bool password_mode,
                                const theme_colors_t *clr)
{
    lv_obj_t *row = lv_obj_create(parent);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_pos(row, 0, y);
    lv_obj_set_style_bg_opa(row, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(row, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(row, 6, LV_PART_MAIN);

    lv_obj_t *lbl = lv_label_create(row);
    lv_label_set_text(lbl, label_text);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_width(lbl, 72);

    lv_obj_t *ta = lv_textarea_create(row);
    lv_obj_set_flex_grow(ta, 1);
    lv_obj_set_height(ta, ITEM_H - 4);
    lv_textarea_set_one_line(ta, true);
    lv_textarea_set_password_mode(ta, password_mode);
    lv_obj_set_style_text_font(ta, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_border_width(ta, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(ta, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_radius(ta, 3, LV_PART_MAIN);
    lv_obj_set_style_pad_left(ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_top(ta, 2, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(ta, 2, LV_PART_MAIN);
    lv_obj_set_style_bg_color(ta, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(ta, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_text_color(ta, clr->text, LV_PART_MAIN);

    return ta;
}

/* ------------------------------------------------------------------ */
/* Helper: create a button with a text label                           */
/* ------------------------------------------------------------------ */

static lv_obj_t *make_btn(lv_obj_t *parent, const char *text,
                           lv_color_t bg, lv_color_t text_color,
                           int w, int h,
                           lv_event_cb_t cb)
{
    lv_obj_t *btn = lv_button_create(parent);
    lv_obj_set_size(btn, w, h);
    lv_obj_set_style_bg_color(btn, bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn, 3, LV_PART_MAIN);
    if (cb) lv_obj_add_event_cb(btn, cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, text);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, text_color, LV_PART_MAIN);
    lv_obj_center(lbl);

    return btn;
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t vault_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating vault UI");

    if (!parent) parent = lv_scr_act();

    lv_obj_update_layout(parent);
    s_app_w = lv_obj_get_width(parent);
    s_app_h = lv_obj_get_height(parent);
    if (s_app_w == 0) s_app_w = 240;
    if (s_app_h == 0) s_app_h = 296;

    memset(&s_vault, 0, sizeof(s_vault));
    s_vault.selected_idx = -1;

    const theme_colors_t *clr = theme_get_colors();

    /* ----------------------------------------------------------------
     * Root container
     * ---------------------------------------------------------------- */
    s_vault.root = lv_obj_create(parent);
    lv_obj_set_size(s_vault.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_vault.root, 0, 0);
    lv_obj_set_style_bg_opa(s_vault.root, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_vault.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_vault.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_vault.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_vault.root, LV_OBJ_FLAG_SCROLLABLE);

    /* ================================================================
     * LOCK SCREEN
     * ================================================================ */
    s_vault.lock_screen = lv_obj_create(s_vault.root);
    lv_obj_set_size(s_vault.lock_screen, s_app_w, s_app_h);
    lv_obj_set_pos(s_vault.lock_screen, 0, 0);
    lv_obj_set_style_bg_color(s_vault.lock_screen, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_vault.lock_screen, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_vault.lock_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_vault.lock_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_vault.lock_screen, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_vault.lock_screen, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(s_vault.lock_screen, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_vault.lock_screen, lock_key_cb, LV_EVENT_KEY, NULL);

    /* Lock icon + title */
    lv_obj_t *lock_title = lv_label_create(s_vault.lock_screen);
    lv_label_set_text(lock_title, "Vault");
    lv_obj_set_style_text_font(lock_title, &lv_font_montserrat_22, LV_PART_MAIN);
    lv_obj_set_style_text_color(lock_title, clr->text, LV_PART_MAIN);
    lv_obj_align(lock_title, LV_ALIGN_TOP_MID, 0, 28);

    /* Password textarea */
    s_vault.master_pw_ta = lv_textarea_create(s_vault.lock_screen);
    lv_obj_set_size(s_vault.master_pw_ta, 220, 28);
    lv_obj_align(s_vault.master_pw_ta, LV_ALIGN_CENTER, 0, -20);
    lv_textarea_set_one_line(s_vault.master_pw_ta, true);
    lv_textarea_set_password_mode(s_vault.master_pw_ta, true);
    lv_textarea_set_placeholder_text(s_vault.master_pw_ta, "Master password");
    lv_obj_set_style_text_font(s_vault.master_pw_ta, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_vault.master_pw_ta, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(s_vault.master_pw_ta, clr->primary, LV_PART_MAIN);
    lv_obj_set_style_radius(s_vault.master_pw_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_bg_color(s_vault.master_pw_ta, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_vault.master_pw_ta, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_vault.master_pw_ta, clr->text, LV_PART_MAIN);
    lv_obj_set_style_pad_left(s_vault.master_pw_ta, 6, LV_PART_MAIN);

    /* Unlock button */
    make_btn(s_vault.lock_screen, "Unlock",
             clr->primary, lv_color_white(),
             80, 26, unlock_btn_cb);
    lv_obj_t *unlock_btn = lv_obj_get_child(s_vault.lock_screen, -1);
    lv_obj_align(unlock_btn, LV_ALIGN_CENTER, 0, 18);

    /* Status label (shows errors) */
    s_vault.unlock_status_lbl = lv_label_create(s_vault.lock_screen);
    lv_label_set_text(s_vault.unlock_status_lbl, "");
    lv_obj_set_style_text_font(s_vault.unlock_status_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_vault.unlock_status_lbl, clr->text_secondary, LV_PART_MAIN);
    lv_obj_align(s_vault.unlock_status_lbl, LV_ALIGN_CENTER, 0, 48);

    /* "First use" hint */
    lv_obj_t *hint_lbl = lv_label_create(s_vault.lock_screen);
    lv_label_set_text(hint_lbl, "First use: sets master password");
    lv_obj_set_style_text_font(hint_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(hint_lbl, clr->text_secondary, LV_PART_MAIN);
    lv_obj_align(hint_lbl, LV_ALIGN_BOTTOM_MID, 0, -8);

    /* Focus the password field */
    lv_group_t *grp = lv_group_get_default();
    if (grp) {
        lv_group_add_obj(grp, s_vault.master_pw_ta);
        lv_group_focus_obj(s_vault.master_pw_ta);
    }

    /* ================================================================
     * LIST SCREEN
     * ================================================================ */
    s_vault.list_screen = lv_obj_create(s_vault.root);
    lv_obj_set_size(s_vault.list_screen, s_app_w, s_app_h);
    lv_obj_set_pos(s_vault.list_screen, 0, 0);
    lv_obj_set_style_bg_color(s_vault.list_screen, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_vault.list_screen, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_vault.list_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_vault.list_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_vault.list_screen, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_vault.list_screen, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(s_vault.list_screen, LV_OBJ_FLAG_HIDDEN | LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_vault.list_screen, list_key_cb, LV_EVENT_KEY, NULL);

    /* List header */
    lv_obj_t *list_hdr = lv_obj_create(s_vault.list_screen);
    lv_obj_set_size(list_hdr, s_app_w, HEADER_H);
    lv_obj_set_pos(list_hdr, 0, 0);
    lv_obj_set_style_bg_color(list_hdr, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(list_hdr, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(list_hdr, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(list_hdr, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(list_hdr, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(list_hdr, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_right(list_hdr, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(list_hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(list_hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(list_hdr, 0, LV_PART_MAIN);
    lv_obj_clear_flag(list_hdr, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(list_hdr, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(list_hdr,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(list_hdr, 6, LV_PART_MAIN);

    lv_obj_t *list_title = lv_label_create(list_hdr);
    lv_label_set_text(list_title, "Vault");
    lv_obj_set_style_text_font(list_title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(list_title, clr->text, LV_PART_MAIN);
    lv_obj_set_flex_grow(list_title, 1);

    /* "+ New" button */
    make_btn(list_hdr, "+ New",
             clr->primary, lv_color_white(),
             48, 22, new_entry_btn_cb);

    /* Lock button */
    make_btn(list_hdr, "L",
             clr->surface, clr->text,
             22, 22, lock_btn_cb);
    lv_obj_t *lock_icon_btn = lv_obj_get_child(list_hdr, -1);
    lv_obj_set_style_border_width(lock_icon_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(lock_icon_btn, clr->text_secondary, LV_PART_MAIN);

    /* Scrollable entry list */
    s_vault.list_container = lv_obj_create(s_vault.list_screen);
    lv_obj_set_pos(s_vault.list_container, 0, HEADER_H);
    lv_obj_set_size(s_vault.list_container, s_app_w, s_app_h - HEADER_H);
    lv_obj_set_style_bg_color(s_vault.list_container, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_vault.list_container, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_vault.list_container, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_vault.list_container, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_vault.list_container, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(s_vault.list_container, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_vault.list_container,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);
    lv_obj_set_scrollbar_mode(s_vault.list_container, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(s_vault.list_container, clr->primary, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_vault.list_container, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_vault.list_container, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(s_vault.list_container, 0, LV_PART_SCROLLBAR);

    /* ================================================================
     * DETAIL / EDIT SCREEN
     * ================================================================ */
    s_vault.detail_screen = lv_obj_create(s_vault.root);
    lv_obj_set_size(s_vault.detail_screen, s_app_w, s_app_h);
    lv_obj_set_pos(s_vault.detail_screen, 0, 0);
    lv_obj_set_style_bg_color(s_vault.detail_screen, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_vault.detail_screen, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_vault.detail_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_vault.detail_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_vault.detail_screen, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_vault.detail_screen, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(s_vault.detail_screen,
                    LV_OBJ_FLAG_HIDDEN | LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_vault.detail_screen, detail_key_cb,
                        LV_EVENT_KEY, NULL);

    /* Detail header */
    lv_obj_t *det_hdr = lv_obj_create(s_vault.detail_screen);
    lv_obj_set_size(det_hdr, s_app_w, HEADER_H);
    lv_obj_set_pos(det_hdr, 0, 0);
    lv_obj_set_style_bg_color(det_hdr, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(det_hdr, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(det_hdr, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(det_hdr, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(det_hdr, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(det_hdr, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(det_hdr, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(det_hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(det_hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(det_hdr, 0, LV_PART_MAIN);
    lv_obj_clear_flag(det_hdr, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(det_hdr, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(det_hdr,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(det_hdr, 6, LV_PART_MAIN);

    /* "< Back" button */
    make_btn(det_hdr, "< Back",
             clr->surface, clr->text,
             48, 22, back_from_detail_cb);
    lv_obj_t *back_btn = lv_obj_get_child(det_hdr, 0);
    lv_obj_set_style_border_width(back_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(back_btn, clr->text_secondary, LV_PART_MAIN);

    /* Spacer label */
    lv_obj_t *det_title = lv_label_create(det_hdr);
    lv_label_set_text(det_title, "Entry");
    lv_obj_set_style_text_font(det_title, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(det_title, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_flex_grow(det_title, 1);

    /* Form body — vertical flex container */
    lv_obj_t *form = lv_obj_create(s_vault.detail_screen);
    lv_obj_set_pos(form, 0, HEADER_H);
    lv_obj_set_size(form, s_app_w, s_app_h - HEADER_H);
    lv_obj_set_style_bg_opa(form, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(form, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(form, 6, LV_PART_MAIN);
    lv_obj_set_style_pad_right(form, 6, LV_PART_MAIN);
    lv_obj_set_style_pad_top(form, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(form, 4, LV_PART_MAIN);
    lv_obj_set_style_radius(form, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_row(form, 2, LV_PART_MAIN);
    lv_obj_set_flex_flow(form, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(form,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);
    lv_obj_set_scrollbar_mode(form, LV_SCROLLBAR_MODE_OFF);

    /* Form fields */
    s_vault.det_name_ta  = make_form_row(form, "Name:",  0,     false, clr);
    s_vault.det_user_ta  = make_form_row(form, "User:",  0,     false, clr);
    s_vault.det_pass_ta  = make_form_row(form, "Pass:",  0,     true,  clr);
    s_vault.det_notes_ta = make_form_row(form, "Notes:", 0,     false, clr);

    /* Password row: append Show/Hide toggle button */
    lv_obj_t *pass_row = lv_obj_get_parent(s_vault.det_pass_ta);
    s_vault.det_pass_toggle_btn = make_btn(pass_row, "Show",
                                            clr->surface, clr->text,
                                            38, ITEM_H - 4,
                                            toggle_pass_cb);
    lv_obj_set_style_border_width(s_vault.det_pass_toggle_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(s_vault.det_pass_toggle_btn,
                                  clr->text_secondary, LV_PART_MAIN);

    /* Separator */
    lv_obj_t *sep = lv_obj_create(form);
    lv_obj_set_size(sep, LV_PCT(100), 1);
    lv_obj_set_style_bg_color(sep, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(sep, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(sep, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(sep, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(sep, 0, LV_PART_MAIN);
    lv_obj_set_style_margin_top(sep, 4, LV_PART_MAIN);
    lv_obj_set_style_margin_bottom(sep, 4, LV_PART_MAIN);

    /* Action buttons row */
    lv_obj_t *btn_row = lv_obj_create(form);
    lv_obj_set_size(btn_row, LV_PCT(100), 28);
    lv_obj_set_style_bg_opa(btn_row, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(btn_row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(btn_row,
                          LV_FLEX_ALIGN_SPACE_BETWEEN,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_clear_flag(btn_row, LV_OBJ_FLAG_SCROLLABLE);

    make_btn(btn_row, "Save",
             clr->primary, lv_color_white(),
             80, 24, save_entry_btn_cb);

    make_btn(btn_row, "Delete",
             clr->surface, clr->text,
             70, 24, delete_entry_btn_cb);
    lv_obj_t *del_btn = lv_obj_get_child(btn_row, -1);
    lv_obj_set_style_border_width(del_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(del_btn, clr->text_secondary, LV_PART_MAIN);

    return ESP_OK;
}

void vault_ui_show(void)
{
    if (s_vault.root) {
        lv_obj_clear_flag(s_vault.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void vault_ui_hide(void)
{
    if (s_vault.root) {
        lv_obj_add_flag(s_vault.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void vault_ui_destroy(void)
{
    if (s_vault.root) {
        lv_obj_delete(s_vault.root);
        s_vault.root = NULL;
    }
}
