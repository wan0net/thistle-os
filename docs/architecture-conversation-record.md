# Recovery, kernel, packaging, boards, and drivers: design conversation record

Status: agreed architecture direction; implementation is incomplete

Conversation captured: 2026-08-02

Last edited: 2026-08-02

This document preserves the full design discussion that began with producing a
T-Deck Pro build and expanded into recovery installation, flash layout, external
system storage, board discovery, T-Deck variants, T-Watch Ultra support,
Rust-only driver reuse, and replacement of the colour-display HAL.

It is a chronological decision record rather than a verbatim chat transcript.
Questions, corrections, rejected alternatives, rationale, risks, and open
questions are retained so a future implementation does not see only the final
diagram and lose why it was chosen.

Detailed companion documents:

- [`recovery-first-installation.md`](recovery-first-installation.md) defines the
  installer, partition candidate, WiFi fallback, serial contract, trust chain,
  package generations, and rollback flow.
- [`rust-driver-inventory.md`](rust-driver-inventory.md) records all current
  Thistle Rust drivers, upstream Rust candidates, gaps, licenses, and excluded
  sources.
- [`display-driver-architecture.md`](display-driver-architecture.md) records the
  decision to replace the current colour display HAL around
  `decaday/display-driver`.

## Final direction in one view

```text
USB/Web Serial flash
        |
        v
ESP-ROM + stock ESP-IDF bootloader
        |
        v
Recovery OS in factory partition
  - serial diagnostics
  - saved/provisioned WiFi attempt
  - encrypted fallback AP + web server
  - chip architecture detection
  - signed architecture-kernel download and verification
        |
        v
Kernel A/B in ota_0 / ota_1
  - safe board/component discovery
  - mount storage-neutral /system volume
  - signed board profile resolution
  - transactional driver/WM/app download
  - load minimum drivers and UI
        |
        v
External system volume
  - SD first
  - NVMe or another backend later
  - versioned package generations and rollback
  - drivers, apps, window managers, themes, detailed configuration
```

The internal flash contains only what is needed to recover and start the kernel,
plus minimal persistent state. Board-specific drivers and applications are not
bundled into Recovery. The kernel is architecture-specific but intended to be
board-independent.

## Agreed principles

1. A user initially flashes one Recovery installer, not a manually assembled
   set of kernel, optional apps, and board drivers.
2. Recovery has a useful serial interface and a web interface.
3. Recovery tries provisioned or saved WiFi for a bounded period, then creates
   an encrypted AP if it cannot connect.
4. Recovery selects only by chip architecture and installs a signed kernel.
5. The kernel resolves the board and fitted components, mounts the system
   volume, and installs signed drivers, window manager, and applications.
6. SD is the first external system-volume backend, not a permanent architectural
   assumption. NVMe or another storage device may replace it later.
7. Internal flash holds the bare minimum: Recovery, two kernel slots where
   practical, NVS/OTA metadata, crash data, and only measured bootstrap state.
8. Large SPIFFS is not a goal. Applications and detailed drivers belong on the
   system volume.
9. Driver and app packages are architecture- and compatibility-aware, signed,
   installed transactionally, and activated by generation.
10. Hardware discovery must be safe and conservative. Ambiguous evidence remains
    unknown rather than silently selecting a convenient board.
11. Reusable driver work should be Rust-only and permissively licensed. C/C++
    vendor projects remain hardware references unless a temporary exception is
    explicitly chosen.
12. ThistleOS is pre-stable. Existing interfaces can be replaced when a better
    model is available; compatibility is not an end in itself.

## Discussion chronology

### 1. A T-Deck Pro build exposed the packaging question

The conversation started with a request for a T-Deck Pro build that would not
take long. A T-Deck Pro build was produced successfully. The local build output
at the time included:

- a roughly 2.3 MB merged `thistle-os-tdeck-pro-full.bin`;
- a roughly 2.2 MB `thistle-os-tdeck-pro-ota.bin`;
- the roughly 2.2 MB application binary.

The immediate follow-up was whether “the build” included optional apps, a
Recovery image, and the kernel. This exposed that one successful firmware build
was not yet a coherent product installer. Several distinct artifact classes had
been conflated:

- bootloader and partition table;
- Recovery OS;
- architecture-specific kernel;
- board-specific drivers;
- minimum and optional apps;
- external storage layout.

The resulting decision was to design the product from the user-facing flash
experience outward, not to call one application binary a complete distribution.

### 2. Recovery became the public installer

The intended installation experience was stated as:

> The recovery OS is what people will flash. It should turn on the web server,
> provide logs over serial, and download the right kernel.

Recovery must accept WiFi information during a local personalization step where
possible. If no valid credentials exist, or connection fails, it creates its own
WiFi network and hosts the same provisioning UI there.

Important consequences:

- serial logging is a supported interface, not incidental debug output;
- the recovery portal is useful both on an existing LAN and on the fallback AP;
- secrets are not compiled into the public image or printed in logs;
- fallback AP credentials may be generated per boot and printed on the physical
  serial console;
- the generic release image remains identical for all boards of the same chip
  architecture unless locally personalized.

The detailed WiFi state machine and serial logging contract are in
`recovery-first-installation.md`.

### 3. “Factory image” and “16 MB flash” were disentangled

There was understandable confusion over whether “factory” meant the whole 16 MB
flash image.

It does not. In ESP-IDF terminology:

- the physical flash chip may be 16 MB;
- `factory` is one application partition inside that address space;
- `ota_0` and `ota_1` are other application partitions;
- NVS, OTA metadata, PHY data, partition table, bootloader, and crash storage
  also consume flash;
- a merged installer file may contain several images placed at their required
  offsets, but that does not make the `factory` partition 16 MB.

The stock ESP32 ROM serial downloader is separate again. It lives in ROM and is
not removed by deleting a “factory reset” feature from ThistleOS. As long as the
board can enter ROM download mode and the flashing pins remain accessible, a
serial flashing tool remains the last-resort recovery path.

This led to an explicit decision: a conventional baseline-kernel factory reset
is not required if the serial flashing path is acceptable. The `factory`
application partition can instead contain Recovery OS.

### 4. Flash layouts were explored rather than treated as settled

Several layouts were discussed.

#### Earlier shape: baseline kernel, Recovery, one update slot

```text
factory = baseline kernel
ota_0   = Recovery
ota_1   = current kernel
```

This retains a traditional factory-reset target but gives the kernel only one
updateable slot. It makes ordinary rollback less robust and was rejected once
ROM serial recovery was accepted as the final emergency path.

#### User-proposed compact shape

The proposal was:

```text
Recovery = 2 MB
ota_0    = 7 MB
ota_1    = 7 MB
```

The intent was correct: shrink Recovery and give the kernel two large A/B slots.
The literal sizes cannot fit as application partitions in a 16 MB flash chip,
because 2 + 7 + 7 MB already consumes all 16 MB before the bootloader,
partition table, NVS, OTA metadata, PHY data, crash data, or alignment gaps.

#### Current candidate, still provisional

The current documented candidate uses:

- 2 MB `factory` Recovery;
- 5.5 MB `ota_0` kernel A;
- 5.5 MB `ota_1` kernel B;
- small NVS, OTA metadata, PHY, and coredump partitions;
- about 2.8 MB left as a provisional bootstrap/reserved ceiling rather than an
  automatically allocated large filesystem.

The observed Recovery binary was about 1.30 MB and the observed kernel about
2.2 MB during the discussion, so the candidate provides growth room. Those
numbers are evidence for a candidate, not a promise that every future build
fits. CI must measure release artifacts before the layout is frozen.

The repository's checked-in root and Recovery partition tables do not yet match
this design. Current files still describe older and inconsistent roles. The
design is documented but not deployed.

### 5. Large SPIFFS was removed from the goal

The question “do we need that large a SPIFFS?” led to a sharper distinction
between internal bootstrap state and the system volume.

The answer is no: not if the design is genuinely external-storage-first.

Internal persistent storage needs only small, high-value state such as:

- Recovery WiFi credentials in a Recovery-owned NVS namespace;
- monotonic anti-rollback floor and trusted key/version state;
- active kernel slot and confirmation state;
- confirmed signed board-profile identity or a non-authoritative board hint;
- active system package generation and last-known-good generation references;
- minimal information needed to find and mount `/system`;
- compact crash/recovery diagnostics.

It should not normally contain:

- application ELFs;
- complete board-driver bundles;
- themes and large UI resources;
- duplicated catalog data;
- arbitrary user data;
- a full fallback copy of the external system volume.

Whether the small state is stored in NVS, a tiny filesystem, or a dedicated
journal is an implementation question. The requirement is bounded,
transactional state—not SPIFFS specifically.

### 6. The filesystem-manager/MMU concern was reframed

The discussion connected flash allocation pressure to the difficulty of memory
management and MMUs on an RTOS. The useful conclusion was that the immediate
problem is primarily package/storage lifecycle, not virtual memory.

An MMU can translate and protect address spaces, but it does not decide:

- which driver packages are installed;
- which generation is active;
- how a failed download resumes;
- how SD and future NVMe backends present the same logical system volume;
- how signed manifests and rollback interact;
- which files are essential enough for internal flash.

Thistle therefore needs a storage/package manager with a stable logical mount,
transactional generations, resumable downloads, verified activation, and
garbage collection. This can be built on RTOS primitives without pretending the
ESP32 has a desktop-style demand-paged virtual memory system.

The separate runtime-memory problem remains real: driver/app ELF allocation,
PSRAM placement, DMA-capable buffers, fragmentation, unload safety, and per-app
limits still require explicit design. It is not solved by naming the package
store SPIFFS.

### 7. The responsibility chain was reduced to three statements

The working summary became:

```text
Recovery downloads kernel
Kernel downloads apps and drivers
Run
```

Expanded responsibility boundaries:

#### Recovery owns

- a reliable bootable rescue environment;
- serial diagnostics;
- WiFi connection and fallback AP provisioning;
- architecture detection;
- signed kernel catalog access;
- kernel download, digest/signature verification, activation, and rollback;
- a web UI sufficient to install or repair the kernel.

Recovery does not own board profiles, display packages, optional apps, or radio
variant policy.

#### Kernel owns

- board/component evidence collection and safe profile resolution;
- the storage-neutral `/system` mount;
- driver, window-manager, and application catalogs;
- signed artifact compatibility and permission policy;
- resumable transactional package installation;
- active/previous package generations;
- loading and supervising drivers/apps;
- the richer user interface and normal network stack.

#### External system volume owns

- board profiles and component package manifests;
- standalone driver ELFs;
- application ELFs;
- window managers and themes;
- larger configuration, indexes, caches, and user data;
- at least the active package generation and a rollback generation when space
  allows.

### 8. “Is that two Recovery partitions?” was answered by capability, not size

The concern was that Recovery downloads a kernel, then the kernel downloads
drivers and apps, which can look like two recovery systems.

The distinction is purpose:

- Recovery is a small, rarely changed root-of-trust installer capable of
  restoring a bootable architecture kernel.
- The kernel is the normal operating environment. It has richer networking,
  storage, package management, permissions, UI, and hardware-service code.

The kernel's ability to provision or repair its external packages is ordinary
OS package management, not a second Recovery OS. Some code may be shared—HTTP,
hashing, manifest parsing, journaling—but the privilege boundary and release
cadence are different.

The design avoids needless duplication by sharing small audited crates where
safe, while keeping Recovery's dependency and capability set intentionally
small.

### 9. Kernel selection is architecture-specific, not board-specific

The kernel package is selected for the ESP32 architecture or chip family, such
as ESP32-S3 or ESP32-C3. It is not a T-Deck Pro kernel or a T-Watch Ultra kernel.

This keeps Recovery generic and makes the kernel reusable across boards with the
same architecture. Once running, the kernel has enough functionality to:

- read saved signed profile state;
- inspect buses and safe identification registers;
- accept a non-authoritative installer hint;
- ask the user when evidence is ambiguous;
- download the matching driver generation.

The kernel is hardware-independent in its core logic. Pin and bus operations
must flow through driver/bus services rather than accumulating direct ESP-IDF
calls in kernel policy modules.

### 10. Pin poking is evidence, not authority

The existing kernel already probes candidate pins and I2C addresses to infer
some boards. This was accepted as a useful starting point but not as sufficient
production identity.

Safe discovery rules:

- prefer previously confirmed, signed board-profile state;
- use read-only, bounded probes only on explicitly permitted candidate pins;
- avoid driving unknown pins to arbitrary levels;
- collect component identities rather than matching one weak address;
- resolve only when exactly one signed catalog profile is compatible;
- treat missing, partial, contradictory, or ambiguous scans as unknown;
- use a flasher-provided board hint only to narrow choices, never to bypass
  signed compatibility;
- persist the confirmed signed profile so normal boots do not repeatedly probe;
- allow deliberate rediscovery after hardware changes.

Known limitations recorded during the discussion:

- the current fallback can uniquely recognize T-Deck Pro from its keyboard and
  BHI260AP evidence on SDA 13/SCL 14;
- the T-Deck and T-Deck Plus fingerprint is currently ambiguous;
- the old code can synthesize an unsigned `board.json`;
- an empty scan can fall back to RAK3312.

The last two behaviours are specifically rejected for production.

### 11. Board families must be component-aware

The discussion clarified that board names alone do not express the complete
hardware:

- a T-Deck Plus variant may add 4G or audio hardware;
- a Max variant may contain both;
- T-Watch Ultra is sold with different radio controllers;
- a single board profile may therefore need confirmed component variants or
  separate signed profiles.

The exact commercial SKU/revision mapping must be verified before manifests are
signed. The architecture should represent components explicitly rather than
hard-code assumptions such as “Plus always means modem” or “Ultra always means
SX1262.”

### 12. T-Watch Ultra became the integration test for the architecture

The request to add T-Watch Ultra work produced a tracker and independently
scoped implementation issues:

- [#120 production support tracker](https://github.com/wan0net/thistle-os/issues/120)
- [#121 signed profile and safe discovery](https://github.com/wan0net/thistle-os/issues/121)
- [#122 XL9555 and AXP2101](https://github.com/wan0net/thistle-os/issues/122)
- [#123 CO5300 and CST9217](https://github.com/wan0net/thistle-os/issues/123)
- [#124 PCF85063A and DRV2605](https://github.com/wan0net/thistle-os/issues/124)
- [#125 MAX98357A and T3902](https://github.com/wan0net/thistle-os/issues/125)
- [#126 ST25R3916 NFC](https://github.com/wan0net/thistle-os/issues/126)
- [#127 radio variants and complete peripheral bring-up](https://github.com/wan0net/thistle-os/issues/127)

This board exercises nearly every architectural boundary:

- ESP32-S3 architecture kernel;
- QSPI AMOLED and capacitive touch;
- shared I2C devices and GPIO expander;
- PMU-controlled rails;
- speaker, PDM microphone, haptics, RTC, IMU, GNSS, NFC, SD;
- shared SPI arbitration;
- multiple radio variants;
- signed component-aware board resolution;
- external driver packages and hardware verification.

It also exposed inaccuracies in the initial bring-up profile: the watch uses
MAX98357A rather than PCM5102A, PCF85063A rather than PCF8563, and CST9217 at the
documented watch address `0x1A` rather than the profile's `0x5A`.

### 13. “Can we stub drivers from elsewhere?” became a Rust-only audit

The first candidate discussed was LilyGoLib. It contains useful support for the
target LilyGo hardware, but it is a C project, not a Rust driver library.

The decision was to search again and retain Rust-only implementations. The
result was not “a crate exists for everything.” It was:

- good permissive candidates exist for many standard controllers;
- the crates must be adapted to Thistle-owned bus services and standalone ELF
  packaging;
- existing local Rust implementations should be compared, not automatically
  discarded;
- BHI260AP, PDM microphone transport, production A7682E modem support, and some
  radio variants still have real gaps;
- GPL projects and crates are excluded even when technically attractive.

The complete device-by-device result is in `rust-driver-inventory.md`.

#### LilyGoLib's retained role

LilyGoLib remains an important reference for:

- component identities;
- board pins and bus topology;
- power and reset ordering;
- controller initialization evidence;
- board/revision/radio variants.

It is not the Rust implementation base.

#### License corrections retained

- `agrucza/uhrwerk-rs` is Rust and contains relevant-looking device work, but it
  is GPL-3.0 and cannot be reused under the repository's no-GPL policy.
- crates.io `iso14443` is GPL-3.0-or-later and is excluded.
- `embassy-rs/rnfc` provides the preferred permissive ST25R3916 direction,
  although it must currently be pinned from Git and may need a Thistle NDEF
  layer.

### 14. `decaday/display-driver` changed the display direction

The `decaday/display-driver` project was identified as a strong basis for the
display work. Its useful properties include:

- pure `no_std` Rust;
- async bus and panel separation;
- explicit areas and frame metadata;
- CO5300 and ST7789 panel crates;
- the exact 410 x 502 CO5300 panel specification relevant to T-Watch Ultra;
- a reusable MIPI DCS layer for future controllers.

The first response proposed keeping it behind the “stable” Thistle display HAL.
That constraint was corrected: ThistleOS is not stable, and there is no reason
to preserve an underspecified interface merely for compatibility.

The accepted direction is therefore:

- replace the current colour-display HAL;
- use `display-driver` as the internal Rust colour-panel framework;
- keep only a new versioned C-compatible ABI at the dynamic `.drv.elf` boundary;
- compile Rust traits and futures inside each driver ELF rather than passing
  them across the loader boundary;
- include buffer length, stride, pixel format, area, flags, capabilities, and
  transfer completion explicitly;
- provide Thistle ESP32 SPI/QSPI/DMA transports over kernel-owned bus services;
- leave e-paper and initially SSD1306 outside the colour operational model.

This correction is important: the ELF ABI is necessary because Rust traits are
not a stable cross-binary ABI, not because the old display vtable deserves to be
kept.

### 15. Display maturity risks were accepted consciously

As reviewed during the discussion, `decaday/display-driver` was at `0.1.x`, had
no tagged release or automated tests in its repository, lacked an ESP32-S3
example, and did not itself provide a complete ESP32 QSPI/DMA transport.

That did not reverse the decision. It changed the adoption method:

- pin an audited version or Git revision;
- start with a recording fake bus and host command-sequence tests;
- use ST7789 as the simpler end-to-end transport/ABI proof;
- then bring up CO5300/QSPI on T-Watch Ultra;
- upstream generic lifecycle, TE, QSPI, or ESP32 improvements where appropriate;
- retain a small adapter or temporary fork rather than exposing upstream types
  in the kernel ABI.

## Package and storage model

### Artifact classes

The release system eventually produces:

| Artifact | Selected by | Stored in | Updated by |
| --- | --- | --- | --- |
| Recovery installer | chip architecture | public merged flash image / `factory` | deliberate serial/Web Serial reflash |
| Kernel image A/B | chip architecture and compatibility | `ota_0` / `ota_1` | Recovery, with signature and rollback checks |
| Board profile | component evidence and user confirmation | minimal confirmed identity internally; full signed profile on `/system` | kernel package manager |
| Driver `.drv.elf` | architecture, ABI, board/component manifest | `/system` generation | kernel package manager |
| Window manager `.wm.elf` | architecture, ABI, user/system choice | `/system` generation | kernel package manager |
| App `.app.elf` | architecture, ABI, permissions, user/system choice | `/system` generation | kernel package manager/app store |
| Themes/assets/config | package/user selection | `/system` | kernel and apps according to ownership |

### Transactional generations

Downloads should be restartable by artifact identity and expected digest. A
candidate generation is assembled without modifying the active generation:

```text
/system/generations/42/   active, verified
/system/generations/43/   downloading/staged
/system/state             active=42 previous=41 candidate=43
```

The exact filesystem layout may change, but the semantics should include:

- per-artifact checkpoints and temporary names;
- size, digest, signature, architecture, ABI, and dependency verification;
- atomic or journaled generation activation;
- boot confirmation and last-known-good rollback;
- garbage collection only after a newer generation is confirmed;
- no deletion of the previous known-good generation merely because a download
  completed.

### Missing or failed external storage

If `/system` is unavailable, the kernel should still provide enough built-in
functionality to explain the problem, use saved WiFi where available, and fetch
or repair the minimum storage driver/package set. Exactly which storage and
network primitives must be compiled into the kernel remains an open bootstrap
question.

The goal is not to duplicate the whole external system internally. It is to
retain the smallest credible path from a verified kernel to a working system
volume.

## Trust and rollback model

```text
ROM/bootloader
    verifies/selects bootable Recovery/kernel partitions

Recovery trust roots
    verify architecture kernel manifest and image

Kernel trust policy
    verifies signed board profiles and component packages

Driver/app manifests
    declare architecture, ABI, capabilities, permissions, dependencies,
    provenance, digest, and signature
```

Open implementation details include secure boot, flash encryption, key rotation,
revocation, anti-rollback storage, and whether Recovery and kernel/package
catalogs use separate signing authorities. The design requires explicit trust
roots and monotonic policy; it does not claim those controls are all deployed.

## Decisions specifically rejected

- Treating the entire 16 MB flash as the `factory` partition.
- Keeping a baseline factory kernel merely to preserve a conventional factory
  reset when ROM serial recovery is acceptable.
- Literal 2 MB Recovery plus two 7 MB kernels in a 16 MB chip without accounting
  for metadata and system partitions.
- A large internal SPIFFS containing normal apps and board-driver bundles.
- Recovery selecting and installing board-specific drivers and apps.
- Recovery and kernel both implementing full system package management.
- Treating the kernel's package-repair capability as a second Recovery OS.
- Hard-coding SD as the permanent system-store interface.
- Inferring a board or radio variant from weak or empty probe evidence.
- Falling back to RAK3312 or another arbitrary profile after failed discovery.
- Preserving the current display HAL solely because it already exists.
- Passing Rust trait objects or futures directly across independently compiled
  ELF boundaries.
- Forcing e-paper, AMOLED/LCD, and monochrome OLED into one operational display
  abstraction.
- Treating LilyGoLib, RadioLib, or vendor Arduino code as Rust drivers.
- Reusing GPL driver code or dependencies in violation of the repository policy.

## Corrections and uncertainty retained

This record intentionally retains places where the discussion corrected an
earlier assumption:

| Earlier assumption or wording | Corrected understanding |
| --- | --- |
| “Factory image” may be the whole 16 MB | `factory` is one application partition; a merged installer contains several flash segments |
| Factory reset is needed for normal ESP32 recovery | ROM serial download remains available independently; baseline-kernel factory reset is optional |
| 2 MB + 7 MB + 7 MB fits a 16 MB product layout | It leaves no room for boot/metadata partitions; current candidate uses 5.5 MB kernel slots |
| Large SPIFFS is needed for apps/drivers | External `/system` holds packages; internal state remains minimal |
| Kernel package installation makes it another Recovery | Kernel package management is normal runtime responsibility; Recovery restores the kernel |
| Recovery should download the right board build | Recovery downloads the right architecture kernel; kernel resolves board/components |
| Existing pin poking identifies boards safely | It is useful evidence but current fallback and unsigned-profile behaviour are unsafe |
| T-Watch Ultra uses PCM5102A/PCF8563/CST9217 at `0x5A` | Correct targets are MAX98357A, PCF85063A, and CST9217 at documented `0x1A` |
| LilyGoLib supplies Rust drivers | It is C and remains a hardware/reference source |
| The existing display HAL is stable | Thistle is pre-stable; the colour HAL should be replaced around the better Rust model |

## Implementation status at capture time

### Existing evidence

- A T-Deck Pro kernel/full image can be built.
- Rust Recovery code already has a web provisioning base, serial logging,
  signed-manifest concepts, and installation journaling.
- Twenty Rust driver modules exist in `kernel_rs`, with differing maturity.
- Board JSON profiles exist for the target devices, including a bring-up
  T-Watch Ultra profile.
- Kernel board-fingerprint code exists as a starting point.
- Standalone driver manifests and a Rust driver template exist.
- T-Watch Ultra issues #120-#127 capture implementation work.

### Not yet complete

- one authoritative 16 MB partition layout shared by all build paths;
- a published merged Recovery-first installer;
- saved-WiFi-first then encrypted-AP fallback behaviour end to end;
- Recovery installing architecture kernel only;
- safe signed component-aware board resolution;
- storage-neutral `/system` and transactional package generations;
- standalone signed packages for the named Rust drivers;
- production BHI260AP, PDM microphone, A7682E Rust, and all radio variants;
- replacement display ABI and ESP32 SPI/QSPI/DMA transports;
- T-Watch Ultra hardware verification;
- complete recovery, kernel, and package rollback evidence.

The documents describe planned or agreed work. They must not be cited as proof
that the feature is deployed or hardware-verified.

## Open questions carried forward

### Flash and bootstrap

- What is the measured maximum Recovery size across release profiles?
- What is the measured kernel ceiling for each architecture?
- Does the provisional internal bootstrap region need a filesystem at all?
- Which minimal network and storage paths must remain compiled into the kernel
  so it can repair `/system`?
- How much coredump storage is operationally useful on 16 MB targets?

### Recovery networking

- How are WiFi credentials provisioned locally without shell history or public
  image leakage?
- How long should Recovery attempt STA before AP fallback?
- Does the captive portal require a DNS redirector in the first release?
- How are Recovery WiFi credentials shared with or deliberately isolated from
  normal kernel profiles?

### Board discovery

- Which read-only evidence uniquely distinguishes T-Deck, Plus, and Max
  revisions?
- Which radio devices provide a safe identity command?
- How are user confirmations recorded and invalidated after hardware changes?
- What is the safe candidate-pin allowlist for each architecture?

### Package system

- What is the manifest schema and dependency solver boundary?
- How are partial downloads checkpointed and resumed by artifact ID/digest?
- What filesystem and atomicity guarantees are required across SD and future
  NVMe backends?
- How many previous generations are retained under low-space conditions?
- What package failure triggers per-driver rollback versus whole-generation
  rollback?

### Dynamic drivers

- Which C-compatible ABI types and operations are frozen first?
- How does the loader prevent unload while callbacks, tasks, interrupts, or DMA
  still reference driver code?
- How are bus, GPIO, rail, and interrupt capabilities granted to drivers?
- How are driver crashes isolated on ESP32 targets without a desktop MMU?

### Display

- Does transfer completion use tokens, callbacks, or a kernel event queue?
- How are TE, compositor pacing, orientation, and touch transforms coordinated?
- Which buffers require copying into internal DMA-capable memory?
- Which lifecycle extensions should be contributed upstream?

### Rust gaps

- Do we implement BHI260AP from datasheets/Bosch material, or can a permissive
  maintained Rust implementation be found?
- What is the viable ESP32 Rust PDM input path for T3902?
- Is A7682E best handled by an `atat`-based new driver, a future A76xx crate, or
  a temporary compatibility package?
- Which common Rust radio crate provides adequate SX1262 and SX1280 behaviour
  without undesirable coupling?

## Next implementation sequence

The discussion implies this order rather than trying to finish every board at
once:

1. Unify and validate the Recovery-first partition table across root, Recovery,
   installer, and CI builds.
2. Make Recovery try saved/provisioned WiFi, fall back to encrypted AP, log the
   complete serial contract, and install only a signed architecture kernel.
3. Define `/system`, minimal internal state, signed package manifests,
   resumable downloads, and generation activation/rollback.
4. Replace unsafe board fallback with signed, conservative component resolution.
5. Define the standalone driver ABI and bus/resource services, including unload
   safety.
6. Prove the new colour display path with ST7789, then CO5300/QSPI.
7. Package existing substantial Rust drivers one at a time, comparing external
   permissive crates and eliminating duplicates.
8. Complete T-Watch Ultra power/expander foundation, then display/touch, then
   RTC/haptics/audio/NFC/radio and full integration.
9. Verify T-Deck Pro, T-Deck/Plus/Max variants, and T-Watch Ultra on hardware
   with retained serial evidence and honest support-state documentation.

## Source and issue references

- Recovery design: [`recovery-first-installation.md`](recovery-first-installation.md)
- Rust drivers: [`rust-driver-inventory.md`](rust-driver-inventory.md)
- Display design: [`display-driver-architecture.md`](display-driver-architecture.md)
- T-Watch Ultra tracker: <https://github.com/wan0net/thistle-os/issues/120>
- `decaday/display-driver`: <https://github.com/decaday/display-driver>
- LilyGoLib: <https://github.com/Xinyuan-LilyGO/LilyGoLib>
- T-Watch Ultra hardware overview: <https://wiki.lilygo.cc/products/t-watch-series/t-watch-ultra/>
