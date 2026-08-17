#!/usr/bin/env python3
"""Offline structural validation for the FileGlyph source package.

This does not replace `cargo test` on Windows. It verifies that the archive is
internally complete, configuration files parse, Rust/PowerShell delimiters are
balanced outside comments and strings, sample ICO directories are well formed,
and no font file was accidentally bundled.
"""

from __future__ import annotations

import json
import re
import struct
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ERRORS: list[str] = []
NOTES: list[str] = []


def fail(message: str) -> None:
    ERRORS.append(message)


def require(path: str) -> Path:
    candidate = ROOT / path
    if not candidate.is_file():
        fail(f"missing required file: {path}")
    return candidate


def parse_data_files() -> list[int]:
    cargo_path = require("Cargo.toml")
    config_path = require("config.example.json")
    try:
        cargo = tomllib.loads(cargo_path.read_text(encoding="utf-8"))
        if cargo.get("package", {}).get("name") != "fileglyph":
            fail("Cargo.toml package name is not fileglyph")
        if cargo.get("package", {}).get("edition") != "2021":
            fail("Cargo.toml edition is not 2021")
    except Exception as error:  # noqa: BLE001 - validation should report all failures
        fail(f"Cargo.toml did not parse: {error}")

    sizes: list[int] = []
    try:
        config = json.loads(config_path.read_text(encoding="utf-8"))
        sizes = [int(value) for value in config["style"]["sizes"]]
        if not sizes or any(size < 1 or size > 256 for size in sizes):
            fail("config.example.json contains an invalid ICO size list")
        if len(sizes) != len(set(sizes)):
            fail("config.example.json contains duplicate ICO sizes")
    except Exception as error:  # noqa: BLE001
        fail(f"config.example.json did not parse: {error}")
    return sorted(sizes)


def rust_raw_string_end(text: str, start: int) -> tuple[int, str] | None:
    """Return (content_start, closing token) for r/raw-byte strings at start."""
    index = start
    if text.startswith("br", index) or text.startswith("cr", index):
        index += 2
    elif text.startswith("r", index):
        index += 1
    else:
        return None

    hashes_start = index
    while index < len(text) and text[index] == "#":
        index += 1
    if index >= len(text) or text[index] != '"':
        return None
    hashes = text[hashes_start:index]
    return index + 1, '"' + hashes


def validate_rust_delimiters(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    stack: list[tuple[str, int]] = []
    pairs = {"}": "{", "]": "[", ")": "("}
    line = 1
    index = 0

    while index < len(text):
        ch = text[index]
        if ch == "\n":
            line += 1
            index += 1
            continue

        if text.startswith("//", index):
            newline = text.find("\n", index + 2)
            if newline < 0:
                break
            index = newline
            continue

        if text.startswith("/*", index):
            depth = 1
            index += 2
            while index < len(text) and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    if text[index] == "\n":
                        line += 1
                    index += 1
            if depth:
                fail(f"{path.relative_to(ROOT)}: unterminated block comment")
            continue

        raw = rust_raw_string_end(text, index)
        if raw is not None:
            content_start, closing = raw
            end = text.find(closing, content_start)
            if end < 0:
                fail(f"{path.relative_to(ROOT)}:{line}: unterminated raw string")
                return
            line += text.count("\n", content_start, end + len(closing))
            index = end + len(closing)
            continue

        if ch == '"':
            index += 1
            while index < len(text):
                if text[index] == "\\":
                    index += 2
                    continue
                if text[index] == '"':
                    index += 1
                    break
                if text[index] == "\n":
                    line += 1
                index += 1
            else:
                fail(f"{path.relative_to(ROOT)}:{line}: unterminated string")
            continue

        if ch == "'":
            # Lifetimes have no closing apostrophe; character literals do. Only
            # skip when an unescaped closing apostrophe appears on this line.
            cursor = index + 1
            escaped = False
            closing = -1
            while cursor < len(text) and text[cursor] != "\n":
                if escaped:
                    escaped = False
                elif text[cursor] == "\\":
                    escaped = True
                elif text[cursor] == "'":
                    closing = cursor
                    break
                cursor += 1
            if closing >= 0:
                index = closing + 1
                continue

        if ch in "{[(":
            stack.append((ch, line))
        elif ch in "}])":
            if not stack or stack[-1][0] != pairs[ch]:
                fail(f"{path.relative_to(ROOT)}:{line}: unmatched {ch}")
                return
            stack.pop()
        index += 1

    if stack:
        opening, opening_line = stack[-1]
        fail(f"{path.relative_to(ROOT)}:{opening_line}: unclosed {opening}")


def validate_powershell_delimiters(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    stack: list[tuple[str, int]] = []
    pairs = {"}": "{", "]": "[", ")": "("}
    line = 1
    index = 0

    while index < len(text):
        ch = text[index]
        if ch == "\n":
            line += 1
            index += 1
            continue
        if ch == "#":
            newline = text.find("\n", index + 1)
            if newline < 0:
                break
            index = newline
            continue
        if ch == "'":
            index += 1
            while index < len(text):
                if text[index] == "'":
                    if index + 1 < len(text) and text[index + 1] == "'":
                        index += 2
                        continue
                    index += 1
                    break
                if text[index] == "\n":
                    line += 1
                index += 1
            continue
        if ch == '"':
            index += 1
            while index < len(text):
                if text[index] == "`":
                    index += 2
                    continue
                if text[index] == '"':
                    index += 1
                    break
                if text[index] == "\n":
                    line += 1
                index += 1
            continue
        if ch in "{[(":
            stack.append((ch, line))
        elif ch in "}])":
            if not stack or stack[-1][0] != pairs[ch]:
                fail(f"{path.relative_to(ROOT)}:{line}: unmatched {ch}")
                return
            stack.pop()
        index += 1

    if stack:
        opening, opening_line = stack[-1]
        fail(f"{path.relative_to(ROOT)}:{opening_line}: unclosed {opening}")


def validate_modules() -> None:
    lib_path = require("src/lib.rs")
    text = lib_path.read_text(encoding="utf-8")
    modules = re.findall(r"^pub mod ([A-Za-z_][A-Za-z0-9_]*);$", text, flags=re.MULTILINE)
    if not modules:
        fail("src/lib.rs did not expose any modules")
    for module in modules:
        flat = ROOT / "src" / f"{module}.rs"
        nested = ROOT / "src" / module / "mod.rs"
        if not flat.is_file() and not nested.is_file():
            fail(f"src/lib.rs declares missing module: {module}")


def read_ico_sizes(path: Path) -> list[int]:
    data = path.read_bytes()
    if len(data) < 6:
        raise ValueError("file is shorter than an ICO header")
    reserved, resource_type, count = struct.unpack_from("<HHH", data, 0)
    if reserved != 0 or resource_type != 1 or count == 0:
        raise ValueError(f"invalid ICO header: reserved={reserved}, type={resource_type}, count={count}")
    directory_end = 6 + count * 16
    if directory_end > len(data):
        raise ValueError("ICO directory exceeds file length")

    sizes: list[int] = []
    for entry_index in range(count):
        offset = 6 + entry_index * 16
        width_byte, height_byte, colors, reserved_byte, planes, bits, byte_count, image_offset = (
            struct.unpack_from("<BBBBHHII", data, offset)
        )
        width = 256 if width_byte == 0 else width_byte
        height = 256 if height_byte == 0 else height_byte
        if width != height:
            raise ValueError(f"entry {entry_index} is not square: {width}x{height}")
        if colors != 0 or reserved_byte != 0:
            raise ValueError(f"entry {entry_index} has invalid directory flags")
        # PNG-compressed ICO entries commonly leave planes/bit-depth as zero in
        # the directory. Validate the actual payload instead of rejecting that.
        if planes > 1 or bits not in {0, 1, 2, 4, 8, 16, 24, 32}:
            raise ValueError(f"entry {entry_index} has implausible planes/bit depth")
        if byte_count == 0 or image_offset < directory_end or image_offset + byte_count > len(data):
            raise ValueError(f"entry {entry_index} payload is outside the file")
        payload = data[image_offset : image_offset + min(byte_count, 8)]
        if payload != b"\x89PNG\r\n\x1a\n" and len(payload) >= 4:
            dib_size = struct.unpack_from("<I", payload, 0)[0]
            if dib_size not in {12, 40, 52, 56, 108, 124}:
                raise ValueError(f"entry {entry_index} is neither PNG nor a recognized DIB")
        sizes.append(width)
    return sorted(sizes)


def validate_samples(expected_sizes: list[int]) -> None:
    sample_dir = ROOT / "samples" / "sample-icons"
    icons = sorted(sample_dir.glob("*.ico"))
    if not icons:
        fail("no sample ICO files were found")
    for icon in icons:
        try:
            sizes = read_ico_sizes(icon)
            if sizes != expected_sizes:
                fail(f"{icon.relative_to(ROOT)} sizes {sizes} do not match {expected_sizes}")
        except Exception as error:  # noqa: BLE001
            fail(f"{icon.relative_to(ROOT)} is invalid: {error}")

    preview = require("samples/preview.png")
    data = preview.read_bytes()
    if len(data) < 24 or data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        fail("samples/preview.png is not a valid PNG header")
    else:
        width, height = struct.unpack_from(">II", data, 16)
        if width < 512 or height < 512:
            fail(f"samples/preview.png is unexpectedly small: {width}x{height}")
        else:
            NOTES.append(f"sample preview: {width}x{height}")
    NOTES.append(f"sample ICO files: {len(icons)}; entries per icon: {len(expected_sizes)}")


def validate_no_bundled_fonts() -> None:
    font_suffixes = {".ttf", ".otf", ".ttc", ".woff", ".woff2", ".eot"}
    bundled = [
        path.relative_to(ROOT)
        for path in ROOT.rglob("*")
        if path.is_file() and path.suffix.lower() in font_suffixes
    ]
    if bundled:
        fail("font files must not be bundled: " + ", ".join(map(str, bundled)))


def main() -> int:
    required = [
        "README.md",
        "DESIGN.md",
        "AGENT-HANDOFF.md",
        "VALIDATION.md",
        "LICENSE",
        "build-windows.ps1",
        "smoke-test.ps1",
        "build-linux.sh",
        "smoke-test.sh",
        "src/main.rs",
        "src/platform/windows.rs",
        "src/platform/linux.rs",
    ]
    for path in required:
        require(path)

    expected_sizes = parse_data_files()
    validate_modules()
    for path in sorted((ROOT / "src").rglob("*.rs")) + sorted((ROOT / "tests").rglob("*.rs")):
        validate_rust_delimiters(path)
    for path in sorted(ROOT.glob("*.ps1")):
        validate_powershell_delimiters(path)
    validate_samples(expected_sizes)
    validate_no_bundled_fonts()

    if ERRORS:
        print("FileGlyph static validation: FAILED", file=sys.stderr)
        for error in ERRORS:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print("FileGlyph static validation: PASS")
    print(f"  Rust files checked: {len(list((ROOT / 'src').rglob('*.rs'))) + len(list((ROOT / 'tests').rglob('*.rs')))}")
    print(f"  ICO sizes: {', '.join(map(str, expected_sizes))}")
    for note in NOTES:
        print(f"  {note}")
    print("  Bundled fonts: none")
    print("  Windows compile/runtime tests: not performed by this script")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
