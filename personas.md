# ThistleOS User Personas

## 1. Cairn -- Search & Rescue Volunteer

**Name:** Ewan MacLeod
**Age:** 34
**Location:** Scottish Highlands
**Device:** T-Deck Pro (e-paper + keyboard)

**Background:** Ewan is a volunteer with Cairngorm Mountain Rescue. He spends weekends and callouts in remote glens with zero mobile coverage. He's technically capable but not a developer -- he wants tools that work out of the box in harsh conditions.

**Why ThistleOS:**
- LoRa mesh messaging lets his team communicate across valleys where no cell signal reaches. He relays positions and status updates between search parties spread over kilometres.
- GPS navigator gives him grid references he can share over LoRa to coordinate convergence on a casualty site.
- E-paper display is readable in direct sunlight and sips battery -- he gets 2+ days on a charge during a multi-day search.
- The physical keyboard lets him type grid refs and messages with gloves on, something touchscreens can't do.
- Encrypted messaging means sensitive casualty information stays private.

**Typical session:** Boots the device at the trailhead. Opens Navigator for bearing and grid ref. Switches to Messenger to check in with base. Sends periodic position updates over LoRa. Receives a relay from another team 8km away via a third device on a ridgeline acting as a mesh node.

---

## 2. Thorn -- Investigative Journalist

**Name:** Amara Osei
**Age:** 29
**Location:** Accra, Ghana / travels frequently
**Device:** T-Deck (LCD + keyboard)

**Background:** Amara covers corruption and extractive industries across West Africa. She's had phones confiscated at borders and knows her regular communications are monitored. She's not paranoid -- she's experienced. She needs a communication device that doesn't look like a smartphone and doesn't run Google or Apple services.

**Why ThistleOS:**
- No cloud accounts, no app store telemetry, no SIM-linked identity when using LoRa or BLE relay.
- Ed25519 signed apps mean she can verify that her tools haven't been tampered with.
- Vault app stores source documents and encryption keys on the SD card, protected by the device's signing infrastructure.
- 4G modem with PPP gives her internet access when she needs it, but she can pull the modem driver entirely when she wants to go dark.
- The device looks like a hobbyist gadget, not a journalist's tool -- it doesn't attract the same attention as a satellite phone.

**Typical session:** At a meeting, she takes notes in the Notes app with the device in her bag, keyboard accessible. Later at a safe location, she connects to WiFi, uses the AI Assistant to help draft questions for a follow-up interview, then sends an encrypted LoRa message to her editor through a relay chain rather than using cellular.

---

## 3. Fern -- Hardware Maker & Driver Author

**Name:** Yuki Tanaka
**Age:** 42
**Location:** Osaka, Japan
**Device:** T3-S3 + custom sensor board / C3-Mini for testing

**Background:** Yuki runs a small workshop making environmental monitoring kits for farmers. She builds custom PCBs with soil moisture sensors, wind gauges, and LoRa backhaul. She's fluent in C and Rust and contributes to open-source embedded projects.

**Why ThistleOS:**
- The HAL vtable architecture means she can write a driver for her custom soil sensor board as a `.drv.elf`, test it on a C3-Mini, and ship it to customers on an SD card -- no recompilation of the OS needed.
- Board config via `board.json` means she defines her custom hardware's pins and buses in a JSON file rather than forking the firmware.
- The driver SDK gives her a clean boundary: implement `driver_init()`, register a vtable, done.
- Recovery OS with I2C/SPI scanning lets her debug new hardware without a JTAG probe -- she can see which devices respond on the bus from the provisioning web UI.
- She uses the App Store infrastructure to distribute her monitoring app to customers alongside the drivers.

**Typical session:** Writes a new Rust driver for an SHT40 humidity sensor. Builds it as a standalone `.drv.elf`. Copies it to an SD card alongside a `board.json` that maps her custom PCB's I2C bus. Boots a C3-Mini with ThistleOS, watches the Recovery OS detect the sensor on I2C scan, then verifies readings in her monitoring app.

---

## 4. Ember -- Off-Grid Field Researcher

**Name:** Dr. Rafael Mendoza
**Age:** 51
**Location:** Amazonas, Brazil (fieldwork) / Sao Paulo (university)
**Device:** LilyGo T-Beam Supreme (ESP32-S3 + GPS + LoRa + solar charging)

**Background:** Rafael is a conservation biologist studying river dolphin populations along tributaries of the Amazon. He spends weeks at a time at remote field stations with generator power and no internet. He needs a device for data logging, team coordination, and occasional satellite-relayed reports.

**Why ThistleOS:**
- LoRa mesh connects his three field stations along a 40km stretch of river. Research assistants send daily observation counts as structured messages.
- GPS logging tracks his boat transects. He exports waypoints from the Navigator to the SD card for later GIS processing.
- The AI Assistant (when he has a satellite internet window) helps him draft Portuguese-to-English summaries of field notes for his international collaborators.
- File Manager on the SD card stores weeks of data without depending on cloud sync.
- Low power consumption means he can charge from a small solar panel at camp.
- The e-book reader lets him carry field guides and species identification PDFs.

**Typical session:** Morning: checks LoRa messages from the upstream station reporting overnight acoustic detections. Loads a GPS transect route in Navigator. During the boat survey, logs sightings in Notes with timestamps. Evening: connects the 4G modem at the station's antenna mast, uses the AI Assistant to translate and summarise the day's notes, then emails the summary via the PPP connection.

---

## 5. Spark -- Cybersecurity Analyst & CTF Player

**Name:** Dani Kovacs
**Age:** 26
**Location:** Budapest, Hungary
**Device:** Heltec WiFi LoRa 32 V3 (ESP32-S3 + LoRa + OLED) + CYD as secondary display

**Background:** Dani works in penetration testing by day and plays CTF competitions on weekends. They carry a T-Deck as a portable RF and network analysis tool alongside their laptop. They're comfortable writing apps and appreciate having a hackable platform that isn't locked down.

**Why ThistleOS:**
- WiFi Scanner app gives quick visibility into nearby networks, channel utilisation, and signal strength during site surveys.
- The LoRa radio with RadioLib lets them experiment with custom protocols, range testing, and signal analysis.
- The Terminal app provides a serial terminal for connecting to routers, switches, and embedded devices via the UART pins.
- The open app ecosystem means they write custom tools as `.app.elf` files -- a BLE beacon scanner, a deauth detector, a packet counter -- and swap them in and out on an SD card.
- The physical keyboard and compact form factor make it usable one-handed while standing in a server room, poking at a rack.
- They run a CYD as a dedicated dashboard showing real-time RF spectrum and network stats, driven by IPC messages from their T-Deck.

**Typical session:** At a client site, boots the T-Deck and runs WiFi Scanner to map the wireless environment. Switches to a custom BLE scanner app to identify IoT devices. Opens Terminal to console into a switch. Later at a CTF, uses LoRa to coordinate with teammates across a venue, sends encrypted flags and hints that can't be sniffed over WiFi.
