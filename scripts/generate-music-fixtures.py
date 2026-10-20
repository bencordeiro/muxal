#!/usr/bin/env python3
"""Regenerate tiny original silence fixtures; needs libsndfile 1.2+ only here.

The app and its tests do not use libsndfile. Test binaries embed these fixtures;
release binaries do not. No downloaded music or recordings are used.
"""
import ctypes as c
import ctypes.util
from pathlib import Path


class Info(c.Structure):
    _fields_ = [
        ("frames", c.c_longlong), ("samplerate", c.c_int),
        ("channels", c.c_int), ("format", c.c_int),
        ("sections", c.c_int), ("seekable", c.c_int),
    ]


lib = c.CDLL(ctypes.util.find_library("sndfile"))
lib.sf_format_check.argtypes = [c.POINTER(Info)]
lib.sf_open.argtypes = [c.c_char_p, c.c_int, c.POINTER(Info)]
lib.sf_open.restype = c.c_void_p
lib.sf_write_short.argtypes = [c.c_void_p, c.POINTER(c.c_short), c.c_longlong]
lib.sf_write_short.restype = c.c_longlong
lib.sf_close.argtypes = [c.c_void_p]

dest = Path(__file__).resolve().parents[1] / "crates/muxal/tests/fixtures/music"
dest.mkdir(parents=True, exist_ok=True)
samples = (c.c_short * 4410)()  # 100 ms of silence, mono, 44.1 kHz.
for extension, format_code in [
    ("wav", 0x010002), ("flac", 0x170002),
    ("ogg", 0x200060), ("mp3", 0x230082),
]:
    info = Info(0, 44100, 1, format_code, 0, 0)
    assert lib.sf_format_check(c.byref(info)), extension
    path = dest / f"silence.{extension}"
    handle = lib.sf_open(str(path).encode(), 0x20, c.byref(info))
    assert handle, extension
    try:
        assert lib.sf_write_short(handle, samples, len(samples)) == len(samples)
    finally:
        assert lib.sf_close(handle) == 0
    print(path.name, path.stat().st_size)
