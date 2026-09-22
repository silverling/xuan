#!/usr/bin/env python3
"""Build native packages from the same staged files as the portable archive."""

import argparse
import hashlib
import os
import platform
import re
import shutil
import struct
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ARCHITECTURES = {"x86_64": ("amd64", 62), "aarch64": ("arm64", 183)}
DEB_LIBRARIES = [
    "libgcc-s1",
    "libvulkan1",
    "libxkbcommon0",
    "libxkbcommon-x11-0",
    "libwayland-client0",
    "libx11-6",
    "libx11-xcb1",
    "libxcb1",
    "libxcursor1",
    "libxi6",
    "libxrandr2",
    "libxfixes3",
]
RPM_LIBRARIES = [
    "libvulkan.so.1",
    "libxkbcommon.so.0",
    "libxkbcommon-x11.so.0",
    "libwayland-client.so.0",
    "libX11.so.6",
    "libX11-xcb.so.1",
    "libxcb.so.1",
    "libXcursor.so.1",
    "libXi.so.6",
    "libXrandr.so.2",
    "libXfixes.so.3",
]


def check_tools(package_format):
    commands = ["readelf"]
    if package_format in ("deb", "all"):
        commands.append("dpkg-deb")
    if package_format in ("rpm", "all"):
        commands.append("rpmbuild")
    missing = [command for command in commands if not shutil.which(command)]
    if missing:
        raise ValueError(f"Missing packaging tools: {', '.join(missing)}")
    if platform.machine() not in ARCHITECTURES:
        raise ValueError("Native packages support x86_64 and aarch64 Linux hosts")


def native_version(version):
    if not re.fullmatch(
        r"\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", version
    ):
        raise ValueError(f"Expected semver without build metadata: {version}")
    return version.replace("-", "~", 1)


def glibc_requirement(binary, architecture):
    with binary.open("rb") as executable:
        header = executable.read(20)
    if (
        header[:6] != b"\x7fELF\x02\x01"
        or struct.unpack("<H", header[18:20])[0] != ARCHITECTURES[architecture][1]
    ):
        raise ValueError(
            "The packaged executable must match the native host architecture"
        )
    versions = subprocess.check_output(
        ["readelf", "--version-info", "--wide", binary], text=True
    )
    glibc = re.findall(r"\bGLIBC_(\d+(?:\.\d+)+)\b", versions)
    if not glibc:
        raise ValueError("Expected a dynamically linked GNU/Linux executable")
    return max(glibc, key=lambda version: tuple(map(int, version.split("."))))


def stage_payload(stage, payload):
    prefix = payload / "usr"
    shutil.copytree(stage / "bin", prefix / "bin")
    shutil.copytree(stage / "share", prefix / "share")
    # Package files must stay readable even when the builder has a private umask.
    payload.chmod(0o755)
    for path in payload.rglob("*"):
        path.chmod(0o755 if path.is_dir() else 0o644)
    (prefix / "bin/xuan").chmod(0o755)


def build_deb(payload, output, version, architecture, glibc):
    # Dynamic desktop libraries are loaded with dlopen and do not appear in ELF NEEDED.
    dynamic = subprocess.check_output(
        ["readelf", "--dynamic", payload / "usr/bin/xuan"], text=True
    )
    needed = set(re.findall(r"Shared library: \[(.*?)\]", dynamic))
    known = {
        "libc.so.6",
        "libm.so.6",
        "libgcc_s.so.1",
        "libpthread.so.0",
        "libdl.so.2",
        "librt.so.1",
        "ld-linux-x86-64.so.2",
        "ld-linux-aarch64.so.1",
    }
    if needed - known:
        raise ValueError(
            f"Add Debian dependency mappings for: {', '.join(sorted(needed - known))}"
        )
    installed_size = sum(
        path.stat().st_size for path in payload.rglob("*") if path.is_file()
    )
    control = payload / "DEBIAN"
    control.mkdir()
    dependencies = ", ".join([f"libc6 (>= {glibc})", *DEB_LIBRARIES])
    (control / "control").write_text(
        f"Package: xuan\nVersion: {version}-1\n"
        f"Architecture: {ARCHITECTURES[architecture][0]}\n"
        "Section: graphics\nPriority: optional\n"
        "Maintainer: Silver Ling <silver.ling@outlook.com>\n"
        "Homepage: https://github.com/silverling/xuan\n"
        f"Installed-Size: {(installed_size + 1023) // 1024}\nDepends: {dependencies}\n"
        "Recommends: xdg-desktop-portal\n"
        "Description: Native Linux image editor\n"
        " Layered compositions, photo retouching, and Nikon RAW development.\n"
    )
    for name in ("postinst", "postrm"):
        shutil.copy2(ROOT / "packaging/refresh-desktop.sh", control / name)
        (control / name).chmod(0o755)
    subprocess.run(
        ["dpkg-deb", "--root-owner-group", "-Zxz", "--build", payload, output],
        check=True,
    )
    shutil.rmtree(control)


def build_rpm(payload, output, version, architecture, temporary):
    spec = temporary / "xuan.spec"
    refresh = (ROOT / "packaging/refresh-desktop.sh").read_text().split("\n", 1)[1]
    requires = "\n".join(f"Requires: {library}()(64bit)" for library in RPM_LIBRARIES)
    spec.write_text(
        f"Name: xuan\nVersion: {version}\nRelease: 1\nBuildArch: {architecture}\n"
        "Summary: Native Linux image editor\n"
        "License: MIT AND LGPL-2.1-only AND OFL-1.1 AND Apache-2.0\n"
        "URL: https://github.com/silverling/xuan\n"
        f"{requires}\nRecommends: xdg-desktop-portal\n"
        "\n%description\n"
        "Layered compositions, photo retouching, and Nikon RAW development.\n"
        '\n%install\nmkdir -p "%{buildroot}"\n'
        'cp -a "%{xuan_payload}/usr" "%{buildroot}/"\n'
        f"\n%post\n{refresh}\n%postun\n{refresh}"
        "\n%files\n%defattr(-,root,root,-)\n/usr/bin/xuan\n"
        "/usr/share/applications/me.silverl.xuan.desktop\n"
        "/usr/share/mime/packages/me.silverl.xuan.xml\n"
        "/usr/share/icons/hicolor/*/apps/me.silverl.xuan.png\n"
        "%doc /usr/share/doc/xuan\n%license /usr/share/licenses/xuan\n"
    )
    subprocess.run(
        [
            "rpmbuild",
            "-bb",
            spec,
            "--define",
            f"_topdir {temporary / 'rpm'}",
            "--define",
            f"_tmppath {temporary}",
            "--define",
            f"_rpmdir {output.parent}",
            "--define",
            f"_rpmfilename {output.name}",
            "--define",
            f"xuan_payload {payload}",
            "--define",
            "debug_package %{nil}",
            "--define",
            "_build_id_links none",
        ],
        check=True,
    )


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-tools", choices=("deb", "rpm", "all"))
    parser.add_argument("format", nargs="?", choices=("deb", "rpm", "all"))
    parser.add_argument("stage", nargs="?", type=Path)
    parser.add_argument("version", nargs="?")
    parser.add_argument("output", nargs="?", type=Path)
    args = parser.parse_args()
    if args.check_tools:
        check_tools(args.check_tools)
        return
    if not all((args.format, args.stage, args.version, args.output)):
        parser.error("format, stage, version, and output are required")
    check_tools(args.format)
    version = native_version(args.version)
    architecture = platform.machine()
    glibc = glibc_requirement(args.stage / "bin/xuan", architecture)
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".native-", dir=args.output) as directory:
        temporary = Path(directory).resolve()
        payload = temporary / "payload"
        stage_payload(args.stage, payload)
        formats = ("deb", "rpm") if args.format == "all" else (args.format,)
        for package_format in formats:
            output = (
                args.output.resolve()
                / f"xuan-{args.version}-linux-{architecture}.{package_format}"
            )
            if package_format == "deb":
                build_deb(payload, output, version, architecture, glibc)
            else:
                build_rpm(payload, output, version, architecture, temporary)
            with output.open("rb") as stream:
                digest = hashlib.file_digest(stream, "sha256").hexdigest()
            output.with_name(output.name + ".sha256").write_text(
                f"{digest}  {output.name}\n"
            )
            print(f"Created {output}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
