// SPDX-License-Identifier: BSD-3-Clause
// Monotonic firmware security-version policy for Recovery activation.

/// Return the last committed firmware security version. A missing value is a
/// fresh-device state; a storage error is not equivalent to a missing value.
pub fn current_version(
    mut load: impl FnMut() -> Result<Option<u32>, String>,
) -> Result<u32, String> {
    load()
        .map(|value| value.unwrap_or(0))
        .map_err(|error| format!("anti-rollback state unavailable: {error}"))
}

/// Require a strictly newer signed firmware security version.
pub fn require_newer(
    candidate: u32,
    load: impl FnMut() -> Result<Option<u32>, String>,
) -> Result<u32, String> {
    if candidate == 0 {
        return Err("firmware security version must be greater than zero".to_string());
    }
    let current = current_version(load)?;
    if candidate <= current {
        return Err(format!(
            "firmware security version {candidate} is not newer than device version {current}"
        ));
    }
    Ok(current)
}

/// Persist and read back the new floor before selecting the firmware for boot.
/// If activation fails or power is lost after the commit, the advanced floor is
/// intentionally retained so the old signed image cannot become eligible again.
pub fn advance_before_activation(
    candidate: u32,
    mut load: impl FnMut() -> Result<Option<u32>, String>,
    mut store: impl FnMut(u32) -> Result<(), String>,
    activate: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    require_newer(candidate, &mut load)?;
    store(candidate).map_err(|error| format!("anti-rollback commit failed: {error}"))?;
    let persisted = current_version(&mut load)?;
    if persisted != candidate {
        return Err(format!(
            "anti-rollback readback mismatch: wrote {candidate}, read {persisted}"
        ));
    }
    activate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[test]
    fn stale_and_replayed_versions_are_rejected_before_activation() {
        for candidate in [499, 500] {
            let activated = Cell::new(false);
            let result = advance_before_activation(
                candidate,
                || Ok(Some(500)),
                |_| panic!("stale version must not be stored"),
                || {
                    activated.set(true);
                    Ok(())
                },
            );
            assert!(result.is_err());
            assert!(!activated.get());
        }
    }

    #[test]
    fn fresh_device_commits_and_reads_back_before_activation() {
        let state = Rc::new(RefCell::new(None));
        let events = Rc::new(RefCell::new(Vec::new()));
        advance_before_activation(
            500,
            {
                let state = state.clone();
                let events = events.clone();
                move || {
                    events.borrow_mut().push("load");
                    Ok(*state.borrow())
                }
            },
            {
                let state = state.clone();
                let events = events.clone();
                move |value| {
                    events.borrow_mut().push("store");
                    *state.borrow_mut() = Some(value);
                    Ok(())
                }
            },
            {
                let events = events.clone();
                move || {
                    events.borrow_mut().push("activate");
                    Ok(())
                }
            },
        )
        .unwrap();
        assert_eq!(*state.borrow(), Some(500));
        assert_eq!(&*events.borrow(), &["load", "store", "load", "activate"]);
    }

    #[test]
    fn unavailable_or_uncommitted_state_fails_closed() {
        let activated = Cell::new(false);
        let read_error = advance_before_activation(
            500,
            || Err("NVS read failed".to_string()),
            |_| Ok(()),
            || {
                activated.set(true);
                Ok(())
            },
        );
        assert!(read_error.is_err());

        let write_error = advance_before_activation(
            500,
            || Ok(None),
            |_| Err("NVS commit failed".to_string()),
            || {
                activated.set(true);
                Ok(())
            },
        );
        assert!(write_error.is_err());
        assert!(!activated.get());
    }

    #[test]
    fn failed_activation_retains_floor_and_blocks_replay() {
        let state = Rc::new(RefCell::new(Some(499)));
        let first = advance_before_activation(
            500,
            {
                let state = state.clone();
                move || Ok(*state.borrow())
            },
            {
                let state = state.clone();
                move |value| {
                    *state.borrow_mut() = Some(value);
                    Ok(())
                }
            },
            || Err("boot selector failed".to_string()),
        );
        assert!(first.is_err());
        assert_eq!(*state.borrow(), Some(500));
        assert!(require_newer(500, || Ok(*state.borrow())).is_err());
    }

    #[test]
    fn readback_mismatch_blocks_activation() {
        let activated = Cell::new(false);
        let result = advance_before_activation(
            501,
            || Ok(Some(500)),
            |_| Ok(()),
            || {
                activated.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!activated.get());
    }
}
