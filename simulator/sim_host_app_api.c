// SPDX-License-Identifier: BSD-3-Clause
// Simulator implementations for host TAP app imports not exported by Rust.

#if defined(__linux__) && !defined(_GNU_SOURCE)
#define _GNU_SOURCE
#endif

#include "sim_vfs.h"
#include "thistle_app.h"

#include <dirent.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <unistd.h>

static int valid_component(const char *value)
{
    if (!value || !value[0] || value[0] == '/') return 0;
    const char *component = value;
    for (const char *p = value;; p++) {
        if (*p == '\\') return 0;
        if (*p == '/' || *p == '\0') {
            size_t length = (size_t)(p - component);
            if (length == 0 || (length == 1 && component[0] == '.') ||
                (length == 2 && component[0] == '.' && component[1] == '.')) return 0;
            if (*p == '\0') break;
            component = p + 1;
        }
    }
    return 1;
}

static int caller_app_id(char id[64])
{
    Dl_info info;
    if (!dladdr(__builtin_return_address(0), &info) || !info.dli_fname) return -1;
    const char *apps = strstr(info.dli_fname, "/apps/");
    if (!apps) return -1;
    apps += 6;
    const char *end = strchr(apps, '/');
    size_t length = end ? (size_t)(end - apps) : strlen(apps);
    if (length == 0 || length >= 64) return -1;
    memcpy(id, apps, length);
    id[length] = '\0';
    return valid_component(id) ? 0 : -1;
}

static int storage_path(const char *relative, char output[1024])
{
    char app_id[64];
    if (caller_app_id(app_id) != 0 || !valid_component(relative)) return -1;
    int written = snprintf(output, 1024, "%s/data/apps/%s/%s",
                           sim_vfs_get_sdcard_path(), app_id, relative);
    return written > 0 && written < 1024 ? 0 : -1;
}

static void ensure_parent(char *path)
{
    for (char *p = path + 1; *p; p++) {
        if (*p == '/') {
            *p = '\0';
            mkdir(path, 0755);
            *p = '/';
        }
    }
}

void thistle_log(const char *tag, const char *message)
{
    printf("[%s] %s\n", tag ? tag : "app", message ? message : "");
}

uint32_t thistle_millis(void)
{
    struct timeval now;
    gettimeofday(&now, NULL);
    return (uint32_t)(((uint64_t)now.tv_sec * 1000u) + (uint64_t)now.tv_usec / 1000u);
}

void *thistle_fs_open(const char *path, const char *mode)
{
    char resolved[1024];
    if (!mode || storage_path(path, resolved) != 0) return NULL;
    if (strchr(mode, 'w') || strchr(mode, 'a')) ensure_parent(resolved);
    return fopen(resolved, mode);
}

int thistle_fs_read(void *buffer, size_t size, size_t count, void *stream)
{
    return stream ? (int)fread(buffer, size, count, (FILE *)stream) : -1;
}

int thistle_fs_write(const void *buffer, size_t size, size_t count, void *stream)
{
    return stream ? (int)fwrite(buffer, size, count, (FILE *)stream) : -1;
}

int thistle_fs_close(void *stream)
{
    return stream ? fclose((FILE *)stream) : -1;
}

static int compare_entries(const void *left, const void *right)
{
    const thistle_fs_entry_t *a = left;
    const thistle_fs_entry_t *b = right;
    return strcmp(a->name, b->name);
}

int thistle_fs_list(const char *path, thistle_fs_entry_t *entries, size_t max_entries)
{
    char resolved[1024];
    if (!entries || max_entries == 0 || storage_path(path, resolved) != 0) return -1;
    DIR *directory = opendir(resolved);
    if (!directory) return -1;
    size_t count = 0;
    struct dirent *item;
    while (count < max_entries && (item = readdir(directory)) != NULL) {
        if (item->d_name[0] == '.') continue;
        char child[1200];
        snprintf(child, sizeof(child), "%s/%s", resolved, item->d_name);
        struct stat metadata;
        if (lstat(child, &metadata) != 0 || S_ISLNK(metadata.st_mode)) continue;
        if (!S_ISREG(metadata.st_mode) && !S_ISDIR(metadata.st_mode)) continue;
        memset(&entries[count], 0, sizeof(entries[count]));
        snprintf(entries[count].name, sizeof(entries[count].name), "%s", item->d_name);
        entries[count].size_bytes = (uint64_t)metadata.st_size;
        entries[count].modified_ms = (uint64_t)metadata.st_mtime * 1000u;
        entries[count].entry_type = S_ISDIR(metadata.st_mode)
            ? THISTLE_FS_TYPE_DIR : THISTLE_FS_TYPE_FILE;
        count++;
    }
    closedir(directory);
    qsort(entries, count, sizeof(entries[0]), compare_entries);
    return (int)count;
}

int thistle_fs_remove(const char *path)
{
    char resolved[1024];
    struct stat metadata;
    if (storage_path(path, resolved) != 0 || lstat(resolved, &metadata) != 0 ||
        !S_ISREG(metadata.st_mode) || S_ISLNK(metadata.st_mode)) return -1;
    return unlink(resolved);
}

int thistle_fs_replace(const char *source, const char *destination)
{
    char from[1024];
    char to[1024];
    if (storage_path(source, from) != 0 || storage_path(destination, to) != 0 ||
        strcmp(from, to) == 0) return -1;
    ensure_parent(to);
    char backup[1100];
    snprintf(backup, sizeof(backup), "%s.thistle-backup", to);
    unlink(backup);
    int had_destination = access(to, F_OK) == 0;
    if (had_destination && rename(to, backup) != 0) return -1;
    if (rename(from, to) != 0) {
        if (had_destination) rename(backup, to);
        return -1;
    }
    if (had_destination) unlink(backup);
    return 0;
}
