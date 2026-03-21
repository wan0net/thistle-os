// SPDX-License-Identifier: BSD-3-Clause
// Kernel version constants

pub const VERSION_MAJOR: u32 = 0;
pub const VERSION_MINOR: u32 = 1;
pub const VERSION_PATCH: u32 = 0;
pub const VERSION_STRING: &str = "0.1.0";

/// Compare a semver requirement string against the running kernel version.
/// Returns true if the requirement is satisfied (req <= current).
pub fn satisfies(requirement: &str) -> bool {
    let mut parts = requirement.split('.');
    let req_major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let req_minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let req_patch: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    if VERSION_MAJOR != req_major {
        return VERSION_MAJOR > req_major;
    }
    if VERSION_MINOR != req_minor {
        return VERSION_MINOR > req_minor;
    }
    VERSION_PATCH >= req_patch
}
