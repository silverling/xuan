#!/usr/bin/env python3
"""Inspect and extract distribution packages without installing them."""

import hashlib
import io
import os
import platform
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
NAME = f"xuan-{VERSION}-linux-{platform.machine()}"


def check_files(root):
    expected = [
        "usr/bin/xuan",
        "usr/share/applications/me.silverl.xuan.desktop",
        "usr/share/mime/packages/me.silverl.xuan.xml",
        "usr/share/licenses/xuan/LICENSE",
        "usr/share/licenses/xuan/rawler-LGPL-2.1.txt",
        "usr/share/licenses/xuan/Inter-LICENSE.txt",
        "usr/share/doc/xuan/source.tar.gz",
        "usr/share/doc/xuan/copyright",
    ]
    for name in expected:
        assert (root / name).is_file(), f"Missing {name}"
    for path in (root / "usr").rglob("*"):
        required = 0o005 if path.is_dir() else 0o004
        assert path.stat().st_mode & required == required, (
            f"Not publicly readable: {path}"
        )
    assert (root / "usr/bin/xuan").stat().st_mode & 0o777 == 0o755
    for original in (ROOT / "assets/icons").glob("hicolor/*/apps/*.png"):
        installed = (
            root / "usr/share/icons" / original.relative_to(ROOT / "assets/icons")
        )
        assert installed.read_bytes() == original.read_bytes(), installed
    assert not list(root.rglob("icon-theme.cache"))
    assert not list(root.rglob("mime.cache"))
    subprocess.run(["desktop-file-validate", root / expected[1]], check=True)
    actual = subprocess.check_output(
        [root / "usr/bin/xuan", "--version"], text=True
    ).strip()
    assert actual == f"xuan {VERSION}", actual
    with tarfile.open(root / "usr/share/doc/xuan/source.tar.gz") as source:
        names = set(source.getnames())
        for name in (
            "Cargo.toml",
            "Cargo.lock",
            "vendor/rawler/Cargo.toml",
            "scripts/package.sh",
            "docs/DEVELOPMENT.md",
        ):
            assert f"source/{name}" in names, f"Missing rebuild source: {name}"


def main():
    os.umask(0o022)
    with tempfile.TemporaryDirectory(prefix="xuan-package-check-") as directory:
        temporary = Path(directory)
        for extension in ("tar.gz", "deb", "rpm"):
            package = ROOT / "dist" / f"{NAME}.{extension}"
            expected_hash = (
                package.with_name(package.name + ".sha256").read_text().split()[0]
            )
            with package.open("rb") as stream:
                assert (
                    hashlib.file_digest(stream, "sha256").hexdigest() == expected_hash
                ), package
            destination = temporary / extension
            if extension == "tar.gz":
                with tarfile.open(package) as archive:
                    archive.extractall(destination, filter="data")
                stage = destination / NAME
                actual = subprocess.check_output(
                    [stage / "bin/xuan", "--version"], text=True
                ).strip()
                assert actual == f"xuan {VERSION}", actual
                assert (stage / "source/vendor/rawler/Cargo.toml").is_file()
                assert (stage / "source/scripts/package-native.py").is_file()
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
            check_files(destination)
            print(
                f"Verified {package.name}: metadata, files, ownership, and executable"
            )
    print("All distribution packages and checksums passed.")


if __name__ == "__main__":
    main()
