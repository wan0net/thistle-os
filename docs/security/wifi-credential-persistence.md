# WiFi credential persistence

WiFi credentials selected through the settings UI are stored in the `wifi`
object of `config/system.json`. The kernel parses the complete document with
`serde_json`, updates only the selected object, and serializes the complete
document again. Credentials are represented as JSON string values rather than
being interpolated into JSON text.

This preserves quotes, backslashes, newlines, and other control characters as
escaped data. An SSID therefore cannot add or replace privileged configuration
keys. Malformed JSON, wrong field types, invalid UTF-8, empty or oversized SSIDs,
and oversized passwords are rejected before the configuration file is written.

Auto-connect uses the same parser, so escaped credentials round-trip to their
original byte strings before they are passed to the WiFi manager. A missing
WiFi object remains a supported configuration, while a malformed enabled WiFi
object fails closed.
