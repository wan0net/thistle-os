# ThistleOS Roadmap

This roadmap consolidates the project's planning into a single document.
It prioritizes polishing and completing existing features over adding new ones.

## Product Direction

Near-term goal:

**ThistleOS beta on T-Deck Pro and T-Deck is reliable, truthful, and updateable.**

That means:

- user-visible features should either work end-to-end or be clearly hidden/marked unsupported
- docs and CI should reflect the real state of the platform
- simulator behavior should be trustworthy enough for day-to-day development

## Roadmap Plan

| Phase | Focus | Priority | Work Items | Acceptance Criteria | Notes |
|---|---|---:|---|---|---|
| 1 | Core claim audit | P0 | Audit every user-visible feature on primary boards and classify as `working`, `partial`, `stubbed`, or `unsupported` | Single support matrix exists for T-Deck Pro and T-Deck; no ambiguous "supported" claims remain | Start here so planning is reality-based |
| 1 | Messenger transport completion | P0 | Finish BLE framing/send/receive; finish Internet transport over WiFi/PPP; make transport availability truthful in UI | Messenger can send/receive over each advertised transport or the UI hides/disables it cleanly | Biggest gap between product story and implementation |
| 1 | WiFi integration hardening | P0 | Wire ESP-IDF event handling into `wifi_manager`; stabilize connect/disconnect/state transitions; verify Settings reads real state | WiFi connect/disconnect works reliably on device; Settings status is accurate; failure modes are user-friendly | Existing feature polish, not net-new |
| 1 | Power + sensor truth pass | P0 | Finish real ADC battery readings; either implement LTR-553 fully or remove/hide it where unsupported | Battery voltage/percent are real on primary hardware; no fake sensor support is exposed | Avoid demo support masquerading as shipped support |
| 1 | Driver lifecycle completion | P1 | Finish unload/start/stop behavior for hot reload on ESP-IDF; validate restart safety | Driver hot-reload works end-to-end on target hardware or is downgraded from done/visible status | Important for platform credibility |
| 1 | ELF permission safety | P1 | Complete syscall permission enforcement; validate pointer checks; add denial-path tests for ELF apps | Unauthorized syscalls are denied; bad pointers are rejected; targeted tests exist | Security hardening already underway |
| 2 | UX consistency pass | P1 | Standardize loading/empty/error/offline states across Launcher, Settings, App Store, Messenger, Navigator, Terminal | Built-in apps use consistent user-facing states and do not surface raw `NOT_SUPPORTED` errors | High polish payoff |
| 2 | Terminal usability polish | P1 | Make the current shell/terminal scaffold dependable before expanding capability; improve missing-command and file-error handling | Terminal is reliably usable for basic diagnostics on device and simulator | Defer major shell expansion until current behavior feels solid |
| 2 | App Store resilience | P1 | Polish install/update/failure/retry paths; verify signature failure behavior and storage edge cases | App installs either succeed or fail with clear state and no broken partial installs | Important trust path |
| 2 | Boot/runtime polish | P1 | Measure boot time, log noise, watchdog margins, task stacks, memory pressure on T-Deck Pro | Clean boot logs, acceptable boot time, no obvious watchdog/stack regressions on primary board | Stabilization work |
| 3 | Simulator parity matrix | P1 | Create explicit parity table for simulator vs hardware by board and subsystem | Developers can tell what the simulator covers and where it diverges | Prevents false confidence |
| 3 | Integration test expansion | P1 | Add tests for board detection, driver loading from `board.json`, app install/signature flow, transport fallback behavior | CI covers main user journeys, not just unit behavior | Build on existing CI strength |
| 3 | Simulator truthfulness | P2 | Hide, label, or emulate unsupported paths in simulator builds instead of silently stubbing them | Simulator behavior matches its documented capabilities | Makes the simulator a reliable dev tool |
| 4 | Documentation reconciliation | P0 | Reconcile README, interface docs, workflows, and support claims; remove stale counts and outdated "done" items | Docs match current implementation and CI behavior | Fast win, high leverage |
| 4 | Release readiness board | P1 | Keep a single beta-readiness checklist in this file instead of scattered status docs | Team has one source of truth for status | Reduces future drift |
| 4 | Beta release gate | P1 | Define and enforce release criteria for T-Deck Pro/T-Deck beta | Named beta gate exists and all roadmap work maps to it | Keeps polish work focused |

## Stage 1 Execution Checklist

This section breaks Stage 1 into the concrete work queue to execute now.

| ID | Task | Status | Deliverable |
|---|---|---|---|
| S1-01 | Support-truth audit for T-Deck Pro and T-Deck | In progress | Public support matrix for core hardware, transports, and user-visible app paths |
| S1-02 | Documentation truth pass | In progress | README and key docs stop claiming unsupported messenger transports and stubbed sensor support as proven |
| S1-03 | Messenger transport work queue | Pending | Actionable subtasks for BLE framing/rx, Internet send/rx, SMS validation, and UI availability gating |
| S1-04 | WiFi integration work queue | Pending | Actionable subtasks for event wiring, connect lifecycle, disconnect lifecycle, and Settings/UI state handling |
| S1-05 | Power and sensor completion decision | Pending | Either implement LTR-553 and finish TP4065B accuracy pass, or downgrade support claims and hide unsupported paths |
| S1-06 | Driver lifecycle completion work queue | Pending | Actionable subtasks for unload, HAL deregister/re-register, restart safety, and on-device validation |
| S1-07 | ELF permission hardening work queue | Pending | Actionable subtasks for permission denial coverage, pointer validation coverage, and unsigned app validation |

### Stage 1 Issued Tasks

| ID | Area | Priority | Scope | Exit Criteria |
|---|---|---:|---|---|
| S1-03A | Messenger BLE | P0 | Define wire format, finish send path, add receive callback path, validate UI availability gating | BLE transport can send and receive end-to-end or is hidden as unavailable |
| S1-03B | Messenger Internet | P0 | Implement readiness check, HTTP/WebSocket send path, receive path, and failure states | Internet transport works end-to-end or is hidden as unavailable |
| S1-03C | Messenger SMS validation | P1 | Validate modem-backed send/receive behavior and user-facing states on primary hardware | SMS path has truthful status and no silent failures |
| S1-03D | Messenger UI truthfulness | P0 | Only expose transports whose backends are actually ready; improve unsupported/error messaging | Messenger UI no longer advertises incomplete transports as working |
| S1-04A | WiFi event wiring | P0 | Connect ESP-IDF event callbacks to `wifi_manager` state transitions | Runtime state reflects actual WiFi events |
| S1-04B | WiFi connect/disconnect lifecycle | P0 | Harden retries, disconnect handling, stale state cleanup, and Settings refresh | Connect/disconnect is reliable and state does not get stuck |
| S1-04C | WiFi simulator and fallback behavior | P1 | Keep simulator behavior explicit and user-facing messaging consistent | Simulator paths are clearly marked and do not pretend to be device-complete |
| S1-05A | TP4065B accuracy pass | P0 | Normalize board-config power wiring into the driver's ADC-channel schema, then verify on-device ADC path, calibration behavior, and percent mapping | Battery readings are believable and stable on primary boards |
| S1-05B | LTR-553 implementation decision | P0 | Finish the Rust driver path and then validate it on T-Deck Pro hardware before upgrading support claims | No remaining mismatch between code and support claim |
| S1-06A | Driver unload safety | P1 | Add real unload behavior, ensure stale HAL pointers are not retained | Hot-reload does not leave dangling pointers |
| S1-06B | Driver restart flow | P1 | Re-register drivers cleanly after stop/start and validate restart path on target | Reload lifecycle works end-to-end on device |
| S1-07A | Syscall permission validation | P1 | Add denial-path tests for restricted symbols and unsigned apps | Permission enforcement is covered by tests |
| S1-07B | Pointer boundary validation | P1 | Add coverage for invalid caller pointers and denied memory regions | Bad pointers are rejected without unsafe behavior |

### Stage 1 Primary-Board Support Matrix

Initial audit snapshot for the beta-hardening cycle:

| Area | T-Deck Pro | T-Deck | Notes |
|---|---|---|---|
| Boot and runtime bring-up | Working | Working | Current mainline focus is runtime board detection and board JSON boot |
| Display/input core path | Working | Working | E-paper and LCD paths are both active |
| LoRa messaging path | Working | Working | Most complete messenger transport today |
| SMS messaging path | Partial | Partial | Present in messenger model but depends on modem path and end-to-end UX validation |
| BLE messaging path | Stubbed | Stubbed | Backend exists as scaffold but send/receive flow is not complete |
| Internet messaging path | Stubbed | Stubbed | Backend exists as scaffold but send/receive flow is not complete |
| WiFi system integration | Partial | Partial | Core support exists, but event/lifecycle hardening remains Stage 1 work |
| Battery reporting | Partial | Partial | Driver exists, but accuracy/real ADC path is still in Stage 1 scope |
| Light sensor | Partial | N/A | Rust LTR-553 path now performs init and reads, but still needs hardware validation on T-Deck Pro |
| Driver hot reload | Partial | Partial | Manager exists, but ESP-IDF unload/start/stop hooks are incomplete |
| ELF app permission enforcement | Partial | Partial | Hardening work is underway locally and needs validation/tests |

## Suggested Sequencing

| Milestone | Duration | Scope |
|---|---:|---|
| Beta Hardening 1 | 2 weeks | Core claim audit, docs reconciliation, messenger transport truth pass, WiFi integration, power driver completion |
| Beta Hardening 2 | 2 weeks | Driver lifecycle, ELF permission hardening, UX consistency, App Store resilience |
| Beta Hardening 3 | 2 weeks | Simulator parity, integration tests, boot/runtime polish, release readiness checklist |
| Beta RC Prep | 1 week | Triage leftovers, fix blockers only, run full validation on T-Deck Pro and T-Deck |

## Definition Of Done

| Area | Done means |
|---|---|
| Primary boards | T-Deck Pro and T-Deck boot reliably and core apps reflect real hardware state |
| Messaging | Only truly working transports are exposed as available |
| Drivers | No "done" lifecycle feature is still stubbed on target |
| UX | Built-in apps fail gracefully and consistently |
| Security | ELF syscall permissions and pointer boundaries are validated |
| Simulator | Documented coverage matches actual behavior |
| Docs | Status docs and workflows agree with reality |

## Deferred Until After This Roadmap

These are intentionally deprioritized while the project is in a polish-first cycle:

- new boards
- new apps
- new assistant/API features
- mesh gateway/base station work
- ambitious WASM enhancements beyond parity and reliability
- major UI expansion unrelated to fixing current rough edges
