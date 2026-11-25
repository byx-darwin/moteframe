"""Offline tests for the FR8029D probe package and its restoration plan."""
import struct
import unittest
import zlib

from pack_probe import build_package, validate_image


def make_stock():
    size = 0x100000
    flash = bytearray(b"\xff" * size)
    flash[0x1000:0x2000] = bytes(4096)
    app = struct.pack("<II", 0x2000B058, 0x10002121) + b"\x31" * 5000
    flash[0x2000:0x2000 + len(app)] = app
    struct.pack_into("<9I", flash, 0, 0xFFFFFFFF, 0, len(app), 0x2000, 0x4000, 0x20,
                     zlib.crc32(app), 0x51525251, 0x00D02E83)
    struct.pack_into("<3I", flash, 36, zlib.crc32(flash[:36]), 0x51525251, 0x8020)
    # Preserve unrelated device-specific state.
    flash[-1] = 0x41
    return bytes(flash)


def make_elf():
    payload = bytearray(628)
    reset = 0x10002141
    struct.pack_into("<II", payload, 0, 0x20008000, reset)
    struct.pack_into("<H", payload, 320, 0xB672)
    elf = bytearray(512) + payload
    struct.pack_into("<16sHHIIIIIHHHHHH", elf, 0, b"\x7fELF\x01\x01\x01",
                     2, 40, 1, reset, 52, 0, 0, 52, 32, 1, 0, 0, 0)
    struct.pack_into("<8I", elf, 52, 1, 512, 0x10002000, 0x10002000, len(payload),
                     len(payload), 5, 512)
    return bytes(elf)


class PackageTests(unittest.TestCase):
    def test_packages_two_sectors_and_restores_full_original(self):
        stock = make_stock()
        candidate, patches = build_package(stock, make_elf())
        self.assertEqual([patch.offset for patch in patches], [0x2000, 0])
        self.assertEqual(candidate[0x1000:0x2000], stock[0x1000:0x2000])
        self.assertEqual(candidate[0x3000:], stock[0x3000:])
        self.assertEqual(candidate[0x2000:0x2000 + 628], make_elf()[512:])
        self.assertEqual(candidate[0x2000 + 628:0x3000], b"\xff" * (4096 - 628))
        self.assertEqual(validate_image(candidate)["payload_bytes"], 628)
        self.assertEqual(struct.unpack_from("<I", candidate, 24)[0], zlib.crc32(make_elf()[512:]))
        self.assertEqual(struct.unpack_from("<I", candidate, 36)[0], zlib.crc32(candidate[:36]))
        restored = bytearray(candidate)
        for patch in patches:
            self.assertEqual(len(patch.original), 4096)
            self.assertEqual(candidate[patch.offset:patch.offset + 4096], patch.replacement)
            restored[patch.offset:patch.offset + 4096] = patch.original
        self.assertEqual(restored, stock)

    def test_rejects_corrupt_header_or_stock_payload(self):
        for offset in (36, 0x2020):
            with self.subTest(offset=offset):
                damaged = bytearray(make_stock())
                damaged[offset] ^= 1
                with self.assertRaisesRegex(ValueError, "CRC"):
                    build_package(bytes(damaged), make_elf())

    def test_rejects_non_empty_ota_sector_and_incompatible_header(self):
        for offset, message in ((0x1000, "OTA"), (44, "boot policy")):
            with self.subTest(offset=offset):
                damaged = bytearray(make_stock())
                damaged[offset] ^= 1
                with self.assertRaisesRegex(ValueError, message):
                    build_package(bytes(damaged), make_elf())

    def test_rejects_wrong_load_address_truncated_segment_or_invalid_vectors(self):
        for offset, word, message in (
            (64, 0x20003000, "XIP"),
            (56, 0xFFFFF000, "segment"),
            (512, 0x20007000, "stack"),
            (516, 0x10002140, "Thumb"),
            (24, 0x10002143, "entry"),
        ):
            with self.subTest(offset=offset):
                broken = bytearray(make_elf())
                struct.pack_into("<I", broken, offset, word)
                with self.assertRaisesRegex(ValueError, message):
                    build_package(make_stock(), bytes(broken))

    def test_includes_contiguous_flash_backed_ram_initializers(self):
        elf = bytearray(make_elf())
        initializer = bytes.fromhex("00207047")
        struct.pack_into("<H", elf, 44, 2)
        struct.pack_into("<8I", elf, 84, 1, len(elf), 0x20003000,
                         0x10002000 + 628, len(initializer), len(initializer), 6, 4)
        elf.extend(initializer)
        candidate, _ = build_package(make_stock(), bytes(elf))
        self.assertEqual(validate_image(candidate)["payload_bytes"], 632)
        self.assertEqual(candidate[0x2000 + 628:0x2000 + 632], initializer)

    def test_rejects_extra_ram_load(self):
        broken = bytearray(make_elf())
        struct.pack_into("<H", broken, 44, 2)
        struct.pack_into("<8I", broken, 84, 1, 0, 0x20003000, 0x20003000, 0, 4, 6, 4)
        with self.assertRaisesRegex(ValueError, "one load"):
            build_package(make_stock(), bytes(broken))
