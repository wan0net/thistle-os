# Third-Party Licenses

ThistleOS is BSD 3-Clause licensed and depends exclusively on Apache-2.0, MIT, and zlib licensed components. No GPL or LGPL code is used.

---

## ESP-IDF

**License:** Apache License 2.0
**Source:** https://github.com/espressif/esp-idf
**Used for:** Build system, FreeRTOS integration, WiFi stack, BLE (NimBLE), mbedTLS, SPIFFS, SD/MMC, esp_lcd, PPP (LwIP), partition management, OTA, nvs_flash.

```
Copyright 2016-2024 Espressif Systems (Shanghai) CO LTD

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0
```

Full text: https://github.com/espressif/esp-idf/blob/master/LICENSE

---

## FreeRTOS

**License:** MIT
**Source:** https://www.freertos.org/
**Used for:** Real-time OS kernel, task scheduling, queues, semaphores. Included via ESP-IDF.

```
Copyright (C) 2021 Amazon.com, Inc. or its affiliates.  All Rights Reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

Full text: https://github.com/FreeRTOS/FreeRTOS-Kernel/blob/main/LICENSE.md

---

## LVGL

**License:** MIT
**Version:** 9.2
**Source:** https://github.com/lvgl/lvgl
**Used for:** UI rendering, widgets, animations, font rendering, touch input handling.

```
MIT License

Copyright (c) 2021 LVGL Kft.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

Full text: https://github.com/lvgl/lvgl/blob/master/LICENCE.txt

---

## esp_lvgl_port

**License:** Apache License 2.0
**Source:** https://github.com/espressif/esp-bsp/tree/master/components/esp_lvgl_port
**Used for:** Integration layer between LVGL 9 and ESP-IDF display/input drivers.

```
Copyright 2023 Espressif Systems (Shanghai) CO LTD

Licensed under the Apache License, Version 2.0.
```

Full text: https://github.com/espressif/esp-bsp/blob/master/LICENSE

---

## RadioLib

**License:** MIT
**Source:** https://github.com/jgromes/RadioLib
**Used for:** SX1262 LoRa radio driver (`drv_radio_sx1262`).

```
MIT License

Copyright (c) 2018 Jan Gromes

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

Full text: https://github.com/jgromes/RadioLib/blob/master/LICENSE

---

## esp_modem

**License:** Apache License 2.0
**Source:** https://github.com/espressif/esp-protocols/tree/master/components/esp_modem
**Used for:** A7682E 4G modem PPP networking (`drv_modem_a7682e`).

```
Copyright 2021-2023 Espressif Systems (Shanghai) CO LTD

Licensed under the Apache License, Version 2.0.
```

Full text: https://github.com/espressif/esp-protocols/blob/master/LICENSE

---

## espressif__elf_loader

**License:** Apache License 2.0
**Source:** https://github.com/espressif/esp-idf-elf-loader
**Used for:** Dynamic ELF loading of apps and drivers from SD card at runtime.

```
Copyright 2023 Espressif Systems (Shanghai) CO LTD

Licensed under the Apache License, Version 2.0.
```

Full text: https://github.com/espressif/esp-idf-elf-loader/blob/main/LICENSE

---

## NimBLE (Apache Mynewt NimBLE)

**License:** Apache License 2.0
**Source:** https://github.com/apache/mynewt-nimble
**Used for:** BLE 5.0 stack. Included via ESP-IDF as `bt/host/nimble`.

```
Copyright 2015-2021 The Apache Software Foundation

Licensed under the Apache License, Version 2.0.
```

Full text: https://github.com/apache/mynewt-nimble/blob/master/LICENSE

---

## mbedTLS

**License:** Apache License 2.0
**Source:** https://github.com/Mbed-TLS/mbedtls
**Used for:** TLS (HTTPS app store), HMAC-SHA256 app signing, AES-256-CBC + PBKDF2-SHA256 (Vault), SHA-256 OTA verification. Included via ESP-IDF.

```
Copyright The Mbed TLS Contributors

Licensed under the Apache License, Version 2.0.
```

Full text: https://github.com/Mbed-TLS/mbedtls/blob/development/LICENSE

---

## SDL2 (Simple DirectMedia Layer)

**License:** zlib License
**Source:** https://github.com/libsdl-org/SDL
**Used for:** Simulator display rendering, keyboard/mouse input, window management. Desktop builds only — not included in firmware.

```
Copyright (C) 1997-2024 Sam Lantinga <slouken@libsdl.org>

This software is provided 'as-is', without any express or implied
warranty.  In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
   claim that you wrote the original software. If you use this software
   in a product, an acknowledgment in the product documentation would be
   appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.
```

Full text: https://github.com/libsdl-org/SDL/blob/main/LICENSE.txt

---

## libcurl

**License:** MIT/X derivative (curl license)
**Source:** https://curl.se/
**Used for:** HTTP/HTTPS client in the desktop simulator (app store, OTA). Not included in firmware.

```
COPYRIGHT AND PERMISSION NOTICE

Copyright (c) 1996 - 2024, Daniel Stenberg, <daniel@haxx.se>, and many
contributors, see the THANKS file.

All rights reserved.

Permission to use, copy, modify, and distribute this software for any purpose
with or without fee is hereby granted, provided that the above copyright
notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF THIRD PARTY RIGHTS. IN
NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE
OR OTHER DEALINGS IN THE SOFTWARE.
```

Full text: https://curl.se/docs/copyright.html

---

## Unity (Test Framework)

**License:** MIT
**Source:** https://github.com/ThrowTheSwitch/Unity
**Used for:** Unit test framework (`components/test_thistle/`). Not included in production firmware.

```
Copyright (c) 2007-2024 Mike Karlesky, Mark VanderVoord, Greg Williams

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

Full text: https://github.com/ThrowTheSwitch/Unity/blob/master/LICENSE.txt

---

## esp-idf-hal (Rust)

**License:** MIT OR Apache-2.0
**Source:** https://github.com/esp-rs/esp-idf-hal
**Used for:** Rust HAL bindings for ESP32-S3 peripherals in the Recovery OS.

Full text: https://github.com/esp-rs/esp-idf-hal/blob/master/LICENSE-MIT
and: https://github.com/esp-rs/esp-idf-hal/blob/master/LICENSE-APACHE

---

## esp-idf-svc (Rust)

**License:** MIT OR Apache-2.0
**Source:** https://github.com/esp-rs/esp-idf-svc
**Used for:** Rust high-level service wrappers (WiFi, HTTP server) in the Recovery OS.

Full text: https://github.com/esp-rs/esp-idf-svc/blob/master/LICENSE-MIT
and: https://github.com/esp-rs/esp-idf-svc/blob/master/LICENSE-APACHE

---

## esp-idf-sys (Rust)

**License:** MIT OR Apache-2.0
**Source:** https://github.com/esp-rs/esp-idf-sys
**Used for:** Low-level Rust bindings to ESP-IDF C APIs in the Recovery OS.

Full text: https://github.com/esp-rs/esp-idf-sys/blob/master/LICENSE-MIT

---

## License Compatibility Summary

All dependencies are compatible with the ThistleOS BSD 3-Clause license:

| License | Compatible with BSD 3-Clause | Components |
|---------|------------------------------|------------|
| Apache-2.0 | Yes | ESP-IDF, esp_lvgl_port, esp_modem, elf_loader, NimBLE, mbedTLS |
| MIT | Yes | LVGL, FreeRTOS, RadioLib, libcurl, Unity, esp-rs crates |
| zlib | Yes | SDL2 |
| MIT OR Apache-2.0 | Yes | esp-idf-hal, esp-idf-svc, esp-idf-sys |

No GPL, LGPL, AGPL, or other copyleft licenses are present in this project.
