// SPDX-License-Identifier: BSD-3-Clause
// Shared ownership of ESP-IDF's process-wide GPIO ISR service.

use core::hint::spin_loop;
use core::sync::atomic::{AtomicU8, Ordering};

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

const UNINITIALIZED: u8 = 0;
const INITIALIZING: u8 = 1;
const READY: u8 = 2;

static STATE: AtomicU8 = AtomicU8::new(UNINITIALIZED);

#[cfg(target_os = "espidf")]
extern "C" {
    fn gpio_install_isr_service(flags: i32) -> i32;
}

/// Install ESP-IDF's global GPIO ISR service at most once.
///
/// `ESP_ERR_INVALID_STATE` means another subsystem installed the service
/// before this shared owner ran; that state is usable and is normalised to
/// success. Other failures leave the helper retryable.
pub(crate) fn ensure_installed(flags: i32) -> i32 {
    loop {
        match STATE.load(Ordering::Acquire) {
            READY => return ESP_OK,
            INITIALIZING => spin_loop(),
            UNINITIALIZED => {
                if STATE
                    .compare_exchange(
                        UNINITIALIZED,
                        INITIALIZING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_err()
                {
                    continue;
                }

                #[cfg(target_os = "espidf")]
                let ret = unsafe { gpio_install_isr_service(flags) };
                #[cfg(not(target_os = "espidf"))]
                let ret = {
                    let _ = flags;
                    ESP_OK
                };

                if ret == ESP_OK || ret == ESP_ERR_INVALID_STATE {
                    STATE.store(READY, Ordering::Release);
                    return ESP_OK;
                }

                STATE.store(UNINITIALIZED, Ordering::Release);
                return ret;
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_ensure_is_idempotent() {
        assert_eq!(ensure_installed(0), ESP_OK);
        assert_eq!(ensure_installed(0), ESP_OK);
        assert_eq!(STATE.load(Ordering::Acquire), READY);
    }
}
