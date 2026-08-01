# App storage isolation

Apps granted `storage` receive a private persistent root at
`/spiffs/data/apps/<manifest-id>/`. The app ABI accepts relative paths only and
maps them beneath that root before calling the filesystem. Absolute paths,
empty paths, `.` and `..` components, unsupported open modes, invalid manifest
IDs, and file handles owned by another app fail closed.

Storage imports are bound to the authenticated loader slot during ELF
relocation. Runtime app tasks do not select their own identity or storage root.
The loader closes outstanding files and clears the binding before a slot is
reused.

There is currently no general shared-storage ABI. Data exchange between apps
must use IPC or a purpose-built privileged broker with an explicit access
policy; granting `storage` does not grant access to system, vault, update, SD
card, or another app's namespace.

## Migration

1. Change app calls to pass relative paths such as `notes/today.txt`; remove
   `/spiffs/` and `/sdcard/` prefixes.
2. A trusted system migration may copy existing files once into
   `/spiffs/data/apps/<manifest-id>/`. Ordinary apps cannot read arbitrary
   legacy locations to migrate themselves.
3. Preserve the manifest ID across upgrades. Changing it intentionally creates
   a new empty storage namespace and requires a trusted migration.
4. Treat a null result from `thistle_fs_open` or `-1` from other file calls as
   an authorization or I/O failure; do not fall back to a process-wide path.
