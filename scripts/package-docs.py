#!/usr/bin/env python3
"""Stage user documentation with links appropriate for installed packages."""

import argparse
import re
from pathlib import Path
from urllib.parse import quote, urlsplit

ROOT = Path(__file__).resolve().parent.parent
DOCUMENTS = {
    "packaging/README.md": "README.md",
    "docs/USAGE.md": "USAGE.md",
    "docs/SHORTCUTS.md": "SHORTCUTS.md",
    "docs/RAW.md": "RAW.md",
    "THIRD_PARTY.md": "THIRD_PARTY.md",
}
LOCAL_LINKS = {
    **DOCUMENTS,
    "packaging/SOURCES.md": "SOURCES.md",
    "LICENSE": "../../licenses/xuan/LICENSE",
    "licenses/rawler-LGPL-2.1.txt": "../../licenses/xuan/rawler-LGPL-2.1.txt",
    "assets/fonts/Inter-LICENSE.txt": "../../licenses/xuan/Inter-LICENSE.txt",
    "vendor/egui-winit/LICENSE-MIT": "../../licenses/xuan/egui-winit/LICENSE-MIT",
    "vendor/egui-winit/LICENSE-APACHE": "../../licenses/xuan/egui-winit/LICENSE-APACHE",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("version")
    args = parser.parse_args()
    args.destination.mkdir(parents=True)
    repository = "https://github.com/silverling/xuan"
    tag = quote(f"v{args.version}", safe="")

    for relative, name in DOCUMENTS.items():
        source = ROOT / relative

        def rewrite_link(match):
            target = urlsplit(match[2])
            if target.scheme or target.netloc or not target.path:
                return match[0]
            path = (source.parent / target.path).resolve().relative_to(ROOT).as_posix()
            destination = LOCAL_LINKS.get(path)
            if destination is None:
                destination = f"{repository}/blob/{tag}/{quote(path)}"
            if target.fragment:
                destination += f"#{target.fragment}"
            return f"{match[1]}{destination})"

        content = re.sub(r"(\[[^\]]*\]\()([^)]+)\)", rewrite_link, source.read_text())
        (args.destination / name).write_text(content)

    archive = f"xuan-{args.version}-source.tar.gz"
    (args.destination / "SOURCES.md").write_text(
        f"# Sources for Xuan {args.version}\n\n"
        f"Download [{archive}]({repository}/releases/download/{tag}/{archive}) "
        f"and its [.sha256 file]({repository}/releases/download/{tag}/{archive}.sha256) "
        f"from the [matching release]({repository}/releases/tag/{tag}).\n\n"
        "This archive contains the matching application sources, patched egui-winit, "
        "and the exact LGPL-licensed Rawler sources used in the binary. "
        "It is provided alongside all binary downloads; retain it when redistributing them. "
        "GitHub's automatic source archives do not contain the vendored Rawler sources.\n\n"
        "To rebuild or relink with a modified Rawler, extract the archive into a writable "
        f"directory, enter `xuan-{args.version}-source`, edit `vendor/rawler` if desired, "
        "and run `cargo build --release --locked`. Build prerequisites and further "
        "instructions are in `docs/DEVELOPMENT.md` inside the source archive.\n"
    )
    (args.destination / "copyright").write_text(
        (ROOT / "LICENSE").read_text()
        + "\nDependency licenses are under share/licenses/xuan in this installation.\n\n"
        + (args.destination / "THIRD_PARTY.md").read_text()
    )


if __name__ == "__main__":
    main()
