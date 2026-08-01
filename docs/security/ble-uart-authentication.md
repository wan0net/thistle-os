# BLE UART authentication

ThistleOS accepts Nordic UART Service (NUS) RX data only from a peer that has
completed authenticated LE Secure Connections pairing with the device.

## Security policy

- The RX characteristic requires both encrypted and authenticated writes.
- NimBLE must negotiate a 16-byte (128-bit) encryption key.
- Legacy pairing is disabled. Secure Connections-only mode and Security Mode 1
  Level 4 are enabled.
- Pairing uses a random six-digit display passkey. While pairing is active, the
  code is shown on the Settings > Bluetooth screen and written to the device
  log for headless development boards.
- Before any peer bytes are copied into an application buffer, ThistleOS reads
  the live NimBLE connection descriptor and independently confirms that the
  connection is encrypted, authenticated, and using a 16-byte key.
- The dispatch boundary also rejects stale connection handles, missing data,
  and unavailable security state before invoking an application callback.

## Client migration

Existing clients that wrote to NUS without pairing are intentionally
incompatible. Remove any stale bond, reconnect, enter the passkey displayed by
ThistleOS, and retry the write after pairing completes. The NUS UUIDs and
application payload framing do not change, and no on-disk migration is needed.

Peers that cannot perform authenticated LE Secure Connections cannot use the
BLE UART transport. Other transports remain unaffected.
