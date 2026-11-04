/* km003c_parse.h — C ABI for the km003c-parse USB PD decoder.
 *
 * All parse functions return a heap-allocated JSON string that the caller
 * MUST free with km003c_free(). The JSON shape is:
 *   {"ok":true, "data": { standard_name, title_zh, summary_zh, msg_id,
 *     spec_rev, power_role, data_role, object_count, extended, detail[],
 *     objects[], wire_hex }}
 * or on failure:
 *   {"ok":false, "error": "..."}
 *
 * The input wire frame is header + objects + extended payload, WITHOUT the
 * trailing 4-byte CRC (same as km003c-core's decode_pd_wire).
 *
 * Panics never cross the boundary; errors are reported in the JSON.
 */
#ifndef KM003C_PARSE_H
#define KM003C_PARSE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Parse a USB PD wire frame from raw bytes. Returns JSON; free with km003c_free. */
char *km003c_parse_pd_wire(const uint8_t *data, size_t len);

/* Parse a USB PD wire frame from a hex string (whitespace / "0x" allowed).
 * Returns JSON; free with km003c_free. */
char *km003c_parse_pd_hex(const char *hex);

/* Free a string returned by this library. */
void km003c_free(char *ptr);

/* Library version string, e.g. "km003c-parse 0.1.0". Free with km003c_free. */
char *km003c_version(void);

#ifdef __cplusplus
}
#endif

#endif /* KM003C_PARSE_H */
