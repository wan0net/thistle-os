# HAL registry generation safety

The HAL registry is owned and synchronized by the Rust kernel. A registration
clones the current registry, changes the private clone, and atomically publishes
the completed immutable generation. Readers therefore see either the complete
old generation or the complete new generation, never a partially updated set of
driver and configuration pointers.

`hal_get_registry()` remains a temporary compatibility view for C callers while
the remaining C kernel code is migrated to Rust. Its returned pointer stays
valid until reboot, but callers should fetch the registry again for each
operation so that newly registered drivers become visible.

Old registry generations and their loaded driver images are retained until
reboot. This is intentional: existing C readers cannot report when they have
finished using a vtable, so unloading the corresponding executable could leave
a valid registry snapshot containing dangling function pointers. The runtime
manager's device-side load, unload, and reload operations therefore return
`ESP_ERR_NOT_SUPPORTED`; ordinary boot loading through the driver loader and HAL
registration continues to work. Physical hot replacement can return once all
registry consumers are Rust-owned and a Rust reader-lifetime mechanism can prove
that a generation is quiescent.

This change does not alter manifests, signatures, stored configuration, or wire
protocols, so no on-disk, cryptographic, or protocol migration is required.
