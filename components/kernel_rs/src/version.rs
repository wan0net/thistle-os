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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_satisfies_exact() {
        // "0.1.0" should be satisfied by the current kernel 0.1.0
        assert!(satisfies("0.1.0"));
    }

    #[test]
    fn test_satisfies_lower() {
        // A requirement lower than the current version must be satisfied
        assert!(satisfies("0.0.1"));
    }

    #[test]
    fn test_satisfies_higher_major() {
        // Requires a higher major version than the kernel provides
        assert!(!satisfies("1.0.0"));
    }

    #[test]
    fn test_satisfies_higher_minor() {
        // Requires a higher minor version within the same major
        assert!(!satisfies("0.2.0"));
    }

    #[test]
    fn test_satisfies_empty() {
        // An empty requirement string parses as 0.0.0 — always satisfied
        assert!(satisfies(""));
    }
}
