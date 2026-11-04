"""python_example.py — call km003c_parse.dll via ctypes.

Run with km003c_parse.dll in the current directory (or on PATH).
"""
import ctypes
import json

dll = ctypes.CDLL("./km003c_parse.dll")
dll.km003c_parse_pd_hex.restype = ctypes.c_void_p
dll.km003c_parse_pd_hex.argtypes = [ctypes.c_char_p]
dll.km003c_free.argtypes = [ctypes.c_void_p]


def parse_hex(hex_str: str):
    ptr = dll.km003c_parse_pd_hex(hex_str.encode("utf-8"))
    if not ptr:
        raise RuntimeError("decoder returned NULL")
    try:
        text = ctypes.string_at(ptr).decode("utf-8")
    finally:
        dll.km003c_free(ptr)
    return json.loads(text)


if __name__ == "__main__":
    # Source_Capabilities with one PPS APDO.
    print(json.dumps(parse_hex("01116421dcc0"), ensure_ascii=False, indent=2))
