// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#include "thistle/manifest.h"

#include <stdio.h>
#include <string.h>

static void populate_manifest(thistle_manifest_t *manifest)
{
    memset(manifest, 0, sizeof(*manifest));
    manifest->type = MANIFEST_TYPE_DRIVER;
    strcpy(manifest->id, "com.thistle.abi-fixture");
    strcpy(manifest->name, "ABI Fixture");
    strcpy(manifest->version, "1.2.3");
    strcpy(manifest->arch, "esp32s3");
    strcpy(manifest->entry, "abi-fixture.drv.elf");
    manifest->permissions = UINT32_C(0xA5A55A5A);
    manifest->background = true;
    manifest->min_memory_kb = UINT32_C(65536);
    strcpy(manifest->hal_interface, "display");
    strcpy(manifest->changelog, "C and Rust agree");
}

static int check_manifest(const thistle_manifest_t *manifest)
{
    return manifest->type == MANIFEST_TYPE_DRIVER &&
           strcmp(manifest->id, "com.thistle.abi-fixture") == 0 &&
           strcmp(manifest->name, "ABI Fixture") == 0 &&
           strcmp(manifest->version, "1.2.3") == 0 &&
           strcmp(manifest->arch, "esp32s3") == 0 &&
           strcmp(manifest->entry, "abi-fixture.drv.elf") == 0 &&
           manifest->permissions == UINT32_C(0xA5A55A5A) &&
           manifest->background &&
           manifest->min_memory_kb == UINT32_C(65536) &&
           strcmp(manifest->hal_interface, "display") == 0 &&
           strcmp(manifest->changelog, "C and Rust agree") == 0;
}

int main(int argc, char **argv)
{
    thistle_manifest_t manifest;
    FILE *file;

    if (argc != 3) {
        return 2;
    }

    if (strcmp(argv[1], "write") == 0) {
        populate_manifest(&manifest);
        file = fopen(argv[2], "wb");
        if (file == NULL) {
            return 3;
        }
        if (fwrite(&manifest, sizeof(manifest), 1, file) != 1) {
            fclose(file);
            return 4;
        }
        return fclose(file) == 0 ? 0 : 5;
    }

    if (strcmp(argv[1], "check") == 0) {
        file = fopen(argv[2], "rb");
        if (file == NULL) {
            return 6;
        }
        if (fread(&manifest, sizeof(manifest), 1, file) != 1) {
            fclose(file);
            return 7;
        }
        if (fclose(file) != 0) {
            return 8;
        }
        return check_manifest(&manifest) ? 0 : 9;
    }

    return 10;
}
