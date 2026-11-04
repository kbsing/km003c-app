/* c_example.c — load km003c_parse.dll dynamically and decode a PD frame.
 *
 * Build (MSVC): cl /Fe:c_example.exe c_example.c
 * Run with km003c_parse.dll next to the exe (or on PATH).
 */
#include <stdio.h>
#include <windows.h>

typedef char *(*ParseHexFn)(const char *hex);
typedef void (*FreeFn)(char *ptr);

int main(void) {
    HMODULE dll = LoadLibraryA("km003c_parse.dll");
    if (!dll) {
        printf("cannot load km003c_parse.dll\n");
        return 1;
    }
    ParseHexFn parse = (ParseHexFn)GetProcAddress(dll, "km003c_parse_pd_hex");
    FreeFn free_str = (FreeFn)GetProcAddress(dll, "km003c_free");
    if (!parse || !free_str) {
        printf("required symbols not found\n");
        FreeLibrary(dll);
        return 1;
    }

    /* Source_Capabilities carrying one PPS APDO (3.30–11.00 V, 5.00 A). */
    char *json = parse("0x0111 64 21 DC C0");
    if (json) {
        printf("%s\n", json);
        free_str(json);
    }

    FreeLibrary(dll);
    return 0;
}
