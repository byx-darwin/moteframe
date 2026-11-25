"""Host-only preparation of traceable external reference assets."""

import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image, PngImagePlugin


def pack_rgb565(image):
    """Encode top-down RGB pixels using the original 5/6/5 channel truncation."""
    output = bytearray()
    for red, green, blue in image.convert("RGB").getdata():
        pixel = ((red >> 3) << 11) | ((green >> 2) << 5) | (blue >> 3)
        output.extend(pixel.to_bytes(2, "big"))
    return bytes(output)


def frame(image, width, height, index):
    """Read a complete frame from a vertical strip; never rescale or pad it."""
    if (width <= 0 or height <= 0 or index < 0 or image.width != width
            or image.height % height or (index + 1) * height > image.height):
        raise ValueError("frame does not fit the source strip")
    return image.crop((0, index * height, width, (index + 1) * height)).convert("RGB")


def prepare(source, output):
    """Prepare the 11 original C widget positions for one fixed visual fixture."""
    # Coordinates and frame extents: UI_win_set_light.c:51-65.
    # Snapshot: Chinese title, 10:50, selected strip frame 3 (= displayed 4).
    sprites = [
        (1, 0, 70, 19, 0, "语言/标题/中英/Dimming-title.bmp"),
        (76, 3, 10, 13, 1, "界面/标题/时间/hj  0  9.bmp"),
        (88, 3, 10, 13, 0, "界面/标题/时间/hj  0  9.bmp"),
        (100, 3, 4, 13, 0, "界面/标题/时间/hj  1.bmp"),
        (104, 3, 10, 13, 5, "界面/标题/时间/hj  0  9.bmp"),
        (115, 3, 10, 13, 0, "界面/标题/时间/hj  0  9.bmp"),
        (0, 49, 128, 24, 0, "界面/亮度调节/lightness_bg.bmp"),
        (59, 33, 10, 11, 2, "界面/亮度调节/lightness_number-small.bmp"),
        (57, 55, 13, 14, 3, "界面/亮度调节/lightness_number.bmp"),
        (59, 83, 10, 11, 4, "界面/亮度调节/lightness_number-small.bmp"),
        (0, 103, 128, 24, 0, "界面/秒表，倒计时/confirm 1.bmp"),
    ]
    output.mkdir(parents=True, exist_ok=True)
    reference = Image.new("RGB", (128, 128), "black")
    manifest = ["scene\t128\t128"]
    provenance = {"kind": "offline static asset fixture", "snapshot": "10:50; displayed level 4",
                  "format": "RGB565 MSB-first; original BMP channel truncation",
                  "source_root": str(source.resolve()), "sprites": []}
    for number, (x, y, width, height, index, relative) in enumerate(sprites):
        original = source / relative
        with Image.open(original) as bitmap:
            crop = frame(bitmap, width, height, index)
        name = f"sprite-{number:02}.rgb565"
        pixels = pack_rgb565(crop)
        (output / name).write_bytes(pixels)
        manifest.append(f"{x}\t{y}\t{width}\t{height}\t{name}")
        reference.paste(crop, (x, y))
        provenance["sprites"].append({"source": relative,
            "source_sha256": hashlib.sha256(original.read_bytes()).hexdigest(),
            "frame": index, "position": [x, y], "size": [width, height],
            "output": name, "output_sha256": hashlib.sha256(pixels).hexdigest()})
    (output / "scene.tsv").write_text("\n".join(manifest) + "\n", encoding="utf-8")
    (output / "reference.rgb565").write_bytes(pack_rgb565(reference))
    reference.save(output / "reference-bmp-composite.png")
    code = source.parents[1] / "FullResources_改复位引脚/Sources/user/win/UI_win_set_light.c"
    provenance["layout_source"] = str(code.resolve())
    provenance["layout_source_sha256"] = hashlib.sha256(code.read_bytes()).hexdigest()
    (output / "provenance.json").write_text(json.dumps(provenance, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Prepared 11 sprites from 7 original BMPs -> {output / 'scene.tsv'}")


def verify(output, rendered):
    """Compare Rust's exported pixels with an independent BMP composition."""
    packed = (output / "reference.rgb565").read_bytes()
    if len(packed) != 128 * 128 * 2:
        raise ValueError("invalid reference pixel count")
    expected = []
    for offset in range(0, len(packed), 2):
        pixel = int.from_bytes(packed[offset:offset + 2], "big")
        expected.append(((pixel >> 11) * 255 // 31, ((pixel >> 5) & 63) * 255 // 63, (pixel & 31) * 255 // 31))
    with Image.open(rendered) as image:
        actual = image.convert("RGB")
    if actual.size != (128, 128):
        raise ValueError("rendered image dimensions differ from the reference")
    mismatches = sum(a != b for a, b in zip(actual.getdata(), expected))
    if mismatches:
        raise ValueError(f"{mismatches} rendered pixels differ from the reference")
    metadata = PngImagePlugin.PngInfo()
    metadata.add_text("Description", "Rust device-ui reference_screen output using original BMP assets; see provenance.json")
    actual.save(output / "actual.png", pnginfo=metadata)
    actual.resize((512, 512), Image.Resampling.NEAREST).save(output / "actual@4x.png", pnginfo=metadata)
    report = {"pixels_compared": 16384, "mismatched_pixels": 0,
              "rendered_ppm_sha256": hashlib.sha256(rendered.read_bytes()).hexdigest()}
    (output / "verification.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, help="original resources/ui_8 folder (read only)")
    parser.add_argument("--output", type=Path, required=True, help="generated fixture directory under target/ui-migration")
    parser.add_argument("--rendered", type=Path, help="verify an existing Rust-generated PPM instead of preparing")
    args = parser.parse_args()
    if args.rendered:
        verify(args.output, args.rendered)
    elif args.source:
        prepare(args.source, args.output)
    else:
        parser.error("--source is required when preparing assets")


if __name__ == "__main__":
    main()
