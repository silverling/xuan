#!/usr/bin/env python3
"""Verify binary payloads and their accompanying rebuildable source archive."""

import argparse
import hashlib
import io
import os
import platform
import re
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent.parent
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"][
    "version"
]
NAME = f"xuan-{VERSION}-linux-{platform.machine()}"
SOURCE_NAME = f"xuan-{VERSION}-source"


def check_checksum(package):
    digest, filename = package.with_name(package.name + ".sha256").read_text().split()
    assert filename == package.name, f"Incorrect checksum filename: {filename}"
    with package.open("rb") as stream:
        assert hashlib.file_digest(stream, "sha256").hexdigest() == digest, package


def check_files(prefix, portable=False, windows=False, appimage=False):
    expected = {
        "share/licenses/xuan/LICENSE",
        "share/licenses/xuan/rawler-LGPL-2.1.txt",
        "share/licenses/xuan/heic-rs-MIT.txt",
        "share/licenses/xuan/tabler-icons-MIT.txt",
        "share/licenses/xuan/Inter-LICENSE.txt",
        "share/licenses/xuan/egui-winit/LICENSE-MIT",
        "share/licenses/xuan/egui-winit/LICENSE-APACHE",
    }
    executable = prefix / ("xuan.exe" if windows else "bin/xuan")
    expected.add(executable.relative_to(prefix).as_posix())
    if not windows:
        expected.update(
            {
                "share/applications/me.silverl.xuan.desktop",
                "share/mime/packages/me.silverl.xuan.xml",
            }
        )
    documents = [
        "README.md",
        "USAGE.md",
        "SHORTCUTS.md",
        "RAW.md",
        "THIRD_PARTY.md",
        "SOURCES.md",
        "copyright",
    ]
    expected.update(f"share/doc/xuan/{name}" for name in documents)
    icons = (
        [] if windows else list((ROOT / "assets/icons").glob("hicolor/*/apps/*.png"))
    )
    expected.update(
        f"share/icons/{path.relative_to(ROOT / 'assets/icons').as_posix()}"
        for path in icons
    )
    if portable:
        expected.add("scripts/install.sh")
    actual = {
        path.relative_to(prefix).as_posix()
        for path in prefix.rglob("*")
        if path.is_file()
    }
    if appimage:
        # linuxdeploy adds shared libraries and their upstream license notices.
        expected.update(
            name
            for name in actual - expected
            if name.startswith(("lib/", "lib64/"))
            or (
                name.startswith(("share/doc/", "share/licenses/"))
                and not name.startswith(("share/doc/xuan/", "share/licenses/xuan/"))
            )
        )
    assert actual == expected, (
        f"Unexpected files: {actual - expected}; missing: {expected - actual}"
    )
    allowed_directories = {parent for name in expected for parent in Path(name).parents}
    if appimage:
        allowed_directories.update(
            Path(name)
            for name in (
                "share/pixmaps",
                "share/icons/hicolor/scalable",
                "share/icons/hicolor/scalable/apps",
            )
        )
    for path in prefix.rglob("*"):
        if appimage and path.is_symlink():
            assert path.resolve().is_relative_to(prefix.resolve()), path
            assert path.exists(), f"Broken symlink: {path}"
        else:
            assert not path.is_symlink(), f"Unexpected symlink: {path}"
        if path.is_dir():
            assert path.relative_to(prefix) in allowed_directories, path
        if not windows:
            required = 0o005 if path.is_dir() else 0o004
            assert path.stat().st_mode & required == required, (
                f"Not publicly readable: {path}"
            )
    if windows:
        data = executable.read_bytes()
        assert data[:2] == b"MZ", "Not a Windows executable"
        pe = int.from_bytes(data[0x3C:0x40], "little")
        assert data[pe : pe + 6] == b"PE\0\0\x64\x86", "Not an x86_64 PE executable"
        assert int.from_bytes(data[pe + 92 : pe + 94], "little") == 2, (
            "Expected a GUI executable"
        )
    else:
        assert executable.stat().st_mode & 0o777 == 0o755
    for original in icons:
        installed = prefix / "share/icons" / original.relative_to(ROOT / "assets/icons")
        assert installed.read_bytes() == original.read_bytes(), installed
    documentation = prefix / "share/doc/xuan"
    for name in documents:
        for link in re.findall(
            r"\[[^\]]*\]\(([^)]+)\)", (documentation / name).read_text(encoding="utf-8")
        ):
            target = urlsplit(link)
            if not target.scheme and not target.netloc and target.path:
                assert (documentation / target.path).exists(), (
                    f"Broken link in {name}: {link}"
                )
    source_notice = (documentation / "SOURCES.md").read_text(encoding="utf-8")
    assert f"/releases/download/v{VERSION}/{SOURCE_NAME}.tar.gz" in source_notice
    assert f"/releases/download/v{VERSION}/{SOURCE_NAME}.tar.gz.sha256" in source_notice
    if not windows:
        subprocess.run(
            [
                "desktop-file-validate",
                prefix / "share/applications/me.silverl.xuan.desktop",
            ],
            check=True,
        )
    if not windows or sys.platform == "win32":
        actual_version = subprocess.check_output(
            [executable, "--version"], text=True, timeout=30
        ).strip()
        assert actual_version == f"xuan {VERSION}", actual_version


def check_source(temporary):
    package = ROOT / "dist" / f"{SOURCE_NAME}.tar.gz"
    check_checksum(package)
    with tarfile.open(package) as archive:
        archive.extractall(temporary, filter="data")
    source = temporary / SOURCE_NAME
    for name in (
        "Cargo.toml",
        "Cargo.lock",
        "vendor/rawler/Cargo.toml",
        "vendor/egui-winit/Cargo.toml",
        "assets/Xuan.png",
        "src/io/fixtures/rgb-strips.heic",
        "src/io/fixtures/checker-grid.heic",
        "licenses/heic-rs-MIT.txt",
        "licenses/tabler-icons-MIT.txt",
        "assets/svg/transform.svg",
        "scripts/package.sh",
        "scripts/package-native.py",
        "scripts/package-windows.py",
        "scripts/package-source.py",
        "packaging/AppRun",
        ".github/workflows/linux.yml",
        ".github/workflows/windows.yml",
        "docs/DEVELOPMENT.md",
        "LICENSE",
        "THIRD_PARTY.md",
    ):
        assert (source / name).is_file(), f"Missing rebuild source: {name}"
    for original in (ROOT / "src").rglob("*"):
        if original.suffix in (".rs", ".wgsl"):
            assert (
                source / original.relative_to(ROOT)
            ).read_bytes() == original.read_bytes(), original
    manifest = tomllib.loads((source / "Cargo.toml").read_text(encoding="utf-8"))
    assert manifest["package"]["version"] == VERSION
    assert manifest["patch"]["crates-io"]["rawler"]["path"] == "vendor/rawler"
    host = (
        subprocess.check_output(["rustc", "-vV"], text=True)
        .split("host: ")[1]
        .splitlines()[0]
    )
    subprocess.run(
        [
            "cargo",
            "metadata",
            "--offline",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            host,
            "--manifest-path",
            source / "Cargo.toml",
        ],
        stdout=subprocess.DEVNULL,
        check=True,
    )
    print(
        f"Verified {package.name}: matching sources, vendored dependencies, resolved lockfile"
    )


def check_appimage(temporary):
    package = ROOT / "dist" / f"{NAME}.AppImage"
    check_checksum(package)
    assert package.stat().st_mode & 0o777 == 0o755
    with package.open("rb") as stream:
        header = stream.read(11)
    assert header[:4] == b"\x7fELF" and header[8:11] == b"AI\x02", (
        "Expected a type 2 AppImage"
    )
    destination = temporary / "AppImage with spaces"
    destination.mkdir()
    subprocess.run(
        [package, "--appimage-extract"],
        cwd=destination,
        stdout=subprocess.DEVNULL,
        check=True,
        timeout=60,
    )
    appdir = destination / "squashfs-root"
    assert {path.name for path in appdir.iterdir()} == {
        "AppRun",
        ".DirIcon",
        "me.silverl.xuan.desktop",
        "me.silverl.xuan.png",
        "usr",
    }
    assert (appdir / "AppRun").read_bytes() == (ROOT / "packaging/AppRun").read_bytes()
    assert (appdir / "AppRun").stat().st_mode & 0o777 == 0o755
    for name in (".DirIcon", "me.silverl.xuan.desktop", "me.silverl.xuan.png"):
        assert (appdir / name).resolve().is_relative_to(appdir), name
        assert (appdir / name).is_file(), name
    assert (appdir / ".DirIcon").read_bytes() == (
        appdir / "me.silverl.xuan.png"
    ).read_bytes()
    subprocess.run(
        ["desktop-file-validate", appdir / "me.silverl.xuan.desktop"], check=True
    )
    check_files(appdir / "usr", appimage=True)
    for name in (
        "libxkbcommon.so.0",
        "libxkbcommon-x11.so.0",
        "libwayland-client.so.0",
    ):
        assert any(
            (appdir / "usr" / lib / name).is_file() for lib in ("lib", "lib64")
        ), f"Missing bundled runtime library: {name}"
    notices = [
        path
        for directory in ("share/doc", "share/licenses")
        for path in (appdir / "usr" / directory).glob("*/*")
        if path.parent.name != "xuan" and path.is_file()
    ]
    assert notices, "Missing bundled library license notices"
    for command in ([appdir / "AppRun"], [package, "--appimage-extract-and-run"]):
        actual_version = subprocess.check_output(
            [*command, "--version"], cwd=destination, text=True, timeout=60
        ).strip()
        assert actual_version == f"xuan {VERSION}", actual_version
    print(
        f"Verified {package.name}: payload, desktop entry, bundled libraries, FUSE-free launch"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    formats = parser.add_mutually_exclusive_group()
    formats.add_argument(
        "--windows", action="store_true", default=sys.platform == "win32"
    )
    formats.add_argument(
        "--appimage", action="store_true", help="check only AppImage and sources"
    )
    args = parser.parse_args()
    os.umask(0o022)
    with tempfile.TemporaryDirectory(prefix="xuan-package-check-") as directory:
        temporary = Path(directory)
        check_source(temporary)
        if args.appimage:
            check_appimage(temporary)
            return
        if args.windows:
            name = f"xuan-{VERSION}-windows-x86_64"
            package = ROOT / "dist" / f"{name}.zip"
            check_checksum(package)
            with zipfile.ZipFile(package) as archive:
                assert archive.testzip() is None, "Corrupt Windows ZIP"
                archive.extractall(temporary / "windows")
            check_files(temporary / "windows" / name, windows=True)
            print(
                f"Verified {package.name}: executable, runtime files, documentation, checksums"
            )
            return
        for extension in ("tar.gz", "deb", "rpm"):
            package = ROOT / "dist" / f"{NAME}.{extension}"
            check_checksum(package)
            destination = temporary / extension
            if extension == "tar.gz":
                with tarfile.open(package) as archive:
                    archive.extractall(destination, filter="data")
                stage = destination / NAME
                check_files(stage, portable=True)
                installed = temporary / "portable installation"
                subprocess.run([stage / "scripts/install.sh", installed], check=True)
                for path in (stage / "share").rglob("*"):
                    if path.is_file():
                        assert (
                            installed / path.relative_to(stage)
                        ).read_bytes() == path.read_bytes(), path
                subprocess.run([installed / "bin/xuan", "--version"], check=True)
                print(
                    f"Verified {package.name}: runtime files, documentation, portable installation"
                )
                continue
            if extension == "deb":
                metadata = subprocess.check_output(
                    ["dpkg-deb", "--field", package], text=True
                )
                assert f"Version: {VERSION.replace('-', '~', 1)}-1\n" in metadata
                assert "libc6 (>= " in metadata and "libvulkan1" in metadata
                controls = subprocess.check_output(
                    ["dpkg-deb", "--ctrl-tarfile", package]
                )
                with tarfile.open(fileobj=io.BytesIO(controls)) as archive:
                    for name in ("./postinst", "./postrm"):
                        assert archive.getmember(name).mode == 0o755
                        assert (
                            b"update-mime-database" in archive.extractfile(name).read()
                        )
                data = subprocess.check_output(["dpkg-deb", "--fsys-tarfile", package])
                with tarfile.open(fileobj=io.BytesIO(data)) as archive:
                    assert all(
                        member.uid == member.gid == 0 for member in archive.getmembers()
                    )
                subprocess.run(
                    ["dpkg-deb", "--extract", package, destination], check=True
                )
            else:
                metadata = subprocess.check_output(
                    ["rpm", "-qp", "--requires", package], text=True
                )
                assert "libc.so.6(GLIBC_" in metadata and "libvulkan.so.1" in metadata
                scripts = subprocess.check_output(
                    ["rpm", "-qp", "--scripts", package], text=True
                )
                assert scripts.count("gtk-update-icon-cache -q -f -t") == 2
                owners = subprocess.check_output(
                    [
                        "rpm",
                        "-qp",
                        "--qf",
                        "[%{FILEUSERNAME}:%{FILEGROUPNAME}\n]",
                        package,
                    ],
                    text=True,
                )
                assert set(owners.splitlines()) == {"root:root"}
                destination.mkdir()
                data = subprocess.check_output(["rpm2cpio", package])
                subprocess.run(
                    [
                        "cpio",
                        "--extract",
                        "--make-directories",
                        "--quiet",
                        "--no-absolute-filenames",
                    ],
                    input=data,
                    cwd=destination,
                    check=True,
                )
            check_files(destination / "usr")
            print(
                f"Verified {package.name}: runtime files, documentation, metadata, ownership"
            )
        check_appimage(temporary)
    print("All binary packages, source archive, and checksums passed.")


if __name__ == "__main__":
    main()
