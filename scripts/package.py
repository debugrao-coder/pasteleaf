#!/usr/bin/env python3
"""Package a Windows release with locked, offline dependency notices."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-pc-windows-msvc"
REQUIRED = {"pasteleaf.exe", "LICENSE", "README.md", "THIRD_PARTY_NOTICES.md", "RELEASE.json"}
FALLBACKS = {("clipboard-win", "5.4.1"): "BSL-1.0.txt"}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + "\n").encode("utf-8")


def cargo_home() -> Path:
    if os.environ.get("CARGO_HOME"):
        return Path(os.environ["CARGO_HOME"]).expanduser().resolve()
    return (Path(os.environ.get("HOME") or Path.home()) / ".cargo").resolve()


def executable(name: str) -> str:
    found = shutil.which(name)
    if found:
        return found
    suffix = ".exe" if os.name == "nt" else ""
    for base in (cargo_home(), Path.home() / ".cargo"):
        candidate = base / "bin" / (name + suffix)
        if candidate.is_file():
            return str(candidate)
    raise ValueError(f"{name} was not found; install the pinned Rust toolchain and add it to PATH")


def run(command: list[str]) -> str:
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, encoding="utf-8")
    if result.returncode:
        raise ValueError(f"Command failed: {' '.join(command)}\n{result.stderr.strip()}")
    return result.stdout


def windows_packages(metadata: dict) -> list[dict]:
    resolve = metadata.get("resolve") or {}
    root = resolve.get("root")
    if not root:
        raise ValueError("Cargo did not return a package dependency graph")
    nodes = {node["id"]: node for node in resolve["nodes"]}
    seen: set[str] = set()
    pending = [root]
    while pending:
        package_id = pending.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        pending.extend(nodes[package_id]["dependencies"])
    return sorted(
        (package for package in metadata["packages"] if package["id"] in seen and package["id"] != root),
        key=lambda package: (package["name"], package["version"]),
    )


def notice_files(crate: Path, declared_file: str | None) -> list[Path]:
    files = {
        path for path in crate.rglob("*")
        if path.is_file() and path.name.lower().startswith(("license", "licence", "copying", "copyright", "notice"))
    }
    if declared_file:
        files.add(crate / declared_file)
    for path in files:
        if not path.resolve().is_relative_to(crate.resolve()):
            raise ValueError("A crate notice file points outside its registry source directory")
        if not path.is_file() or not path.stat().st_size:
            raise ValueError(f"Missing or empty crate notice file: {path.name}")
    return sorted(files, key=lambda path: path.relative_to(crate).as_posix())


def dependency_notices(metadata: dict, payload: dict[str, bytes]) -> list[dict]:
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    locked = {(package["name"], package["version"], package.get("source")): package for package in lock["package"]}
    registry = (cargo_home() / "registry" / "src").resolve()
    fallback_sources = json.loads((ROOT / "license_assets" / "SOURCES.json").read_text(encoding="utf-8"))
    inventory = []
    for package in windows_packages(metadata):
        name, version = package["name"], package["version"]
        source = package.get("source") or ""
        if not source.startswith("registry+") or not package.get("license"):
            raise ValueError(f"Review source and licensing before packaging {name} {version}")
        crate = Path(package["manifest_path"]).parent.resolve()
        if not crate.is_relative_to(registry):
            raise ValueError(f"{name} {version} is outside CARGO_HOME/registry/src")
        archive_checksum = locked[(name, version, source)]["checksum"]
        cache_file = cargo_home() / "registry" / "cache" / crate.parent.name / f"{name}-{version}.crate"
        if not cache_file.is_file():
            raise ValueError(f"Locked crate archive is missing from Cargo cache: {name} {version}; run cargo fetch --locked --target {TARGET}")
        archive_data = cache_file.read_bytes()
        if digest(archive_data) != archive_checksum:
            raise ValueError(f"Registry archive checksum does not match Cargo.lock: {name} {version}")
        record = {
            "name": name,
            "version": version,
            "license_expression": package["license"],
            "repository": package.get("repository"),
            "registry": source,
            "crate_archive_sha256": archive_checksum,
            "license_files": [],
        }
        files = notice_files(crate, package.get("license_file"))
        with tarfile.open(fileobj=io.BytesIO(archive_data), mode="r:gz") as cached_archive:
            for path in files:
                relative = path.relative_to(crate).as_posix()
                data = path.read_bytes()
                member = cached_archive.getmember(f"{name}-{version}/{relative}")
                if not member.isfile():
                    raise ValueError(f"Registry notice is not a regular archive file: {name} {version}/{relative}")
                original = cached_archive.extractfile(member)
                if original is None or data != original.read():
                    raise ValueError(f"Modified registry notice: {name} {version}/{relative}")
                destination = f"third-party/{name}-{version}/{relative}"
                payload[destination] = data
                record["license_files"].append({"path": destination, "sha256": digest(data), "origin": "crate_archive"})
        if not files:
            fallback = FALLBACKS.get((name, version))
            if not fallback:
                raise ValueError(f"No license text found for {name} {version}; add a reviewed, pinned fallback")
            attribution = fallback_sources[fallback]
            if package["license"] != attribution["license"] or {"name": name, "version": version} not in attribution["used_by"]:
                raise ValueError(f"Fallback license does not match {name} {version}")
            data = (ROOT / "license_assets" / fallback).read_bytes()
            if digest(data) != attribution["sha256"]:
                raise ValueError(f"Vendored license hash does not match its official source: {fallback}")
            destination = f"third-party/{name}-{version}/{fallback}"
            payload[destination] = data
            record["license_files"].append({
                "path": destination,
                "sha256": digest(data),
                "origin": "vendored_official_license",
                "source": attribution["source"],
                "reason": attribution["reason"],
            })
        inventory.append(record)
    return inventory


def rust_notices(payload: dict[str, bytes]) -> str:
    rustc = executable("rustc")
    sysroot = Path(run([rustc, "--print", "sysroot"]).strip())
    docs = sysroot / "share" / "doc" / "rust"
    copyright_file = docs / "COPYRIGHT-library.html"
    license_dir = docs / "licenses"
    if not copyright_file.is_file() or not (license_dir / "MIT.txt").is_file() or not (license_dir / "Apache-2.0.txt").is_file():
        raise ValueError("Rust standard-library copyright/license files are missing from the pinned toolchain")
    payload["third-party/rust/COPYRIGHT-library.html"] = copyright_file.read_bytes()
    for path in sorted(license_dir.rglob("*")):
        if path.is_file():
            if not path.resolve().is_relative_to(docs.resolve()):
                raise ValueError("A Rust license points outside the toolchain documentation")
            payload["third-party/rust/" + path.relative_to(docs).as_posix()] = path.read_bytes()
    return run([rustc, "--version"]).strip()


def validate_exe(data: bytes) -> None:
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("Release executable is not a Windows PE file")
    pe_offset = struct.unpack_from("<I", data, 0x3C)[0]
    if pe_offset + 6 > len(data) or data[pe_offset:pe_offset + 4] != b"PE\0\0" or struct.unpack_from("<H", data, pe_offset + 4)[0] != 0x8664:
        raise ValueError("Release executable must target Windows x64")


def safe_member(name: str) -> bool:
    path = PurePosixPath(name)
    return bool(name) and "\\" not in name and ":" not in name and not path.is_absolute() and ".." not in path.parts and str(path) == name


def verify_package(path: Path, check_sidecar: bool = True) -> dict:
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        name_set = set(names)
        if len(name_set) != len(names) or any(not safe_member(name) for name in names):
            raise ValueError("ZIP contains duplicate or unsafe file paths")
        if not (REQUIRED | {"SHA256SUMS", "third-party/DEPENDENCIES.json", "third-party/rust/COPYRIGHT-library.html", "third-party/rust/licenses/MIT.txt", "third-party/rust/licenses/Apache-2.0.txt"}).issubset(name_set):
            raise ValueError("ZIP is missing a required executable, document, or notice")
        if any(name not in REQUIRED | {"SHA256SUMS"} and not name.startswith("third-party/") for name in names):
            raise ValueError("ZIP contains unexpected application data or build files")
        if archive.testzip():
            raise ValueError("ZIP failed its CRC check")
        checksums = {}
        for line in archive.read("SHA256SUMS").decode("utf-8").splitlines():
            checksum, member = line.split("  ", 1)
            if member in checksums or len(checksum) != 64 or any(char not in "0123456789abcdef" for char in checksum):
                raise ValueError("ZIP hash manifest contains invalid or duplicate entries")
            checksums[member] = checksum
        if set(checksums) != name_set - {"SHA256SUMS"}:
            raise ValueError("ZIP hash manifest does not cover every payload file")
        for member, checksum in checksums.items():
            if digest(archive.read(member)) != checksum:
                raise ValueError(f"ZIP hash mismatch: {member}")
        validate_exe(archive.read("pasteleaf.exe"))
        release = json.loads(archive.read("RELEASE.json"))
        dependencies = json.loads(archive.read("third-party/DEPENDENCIES.json"))
        if release["target"] != TARGET or release["third_party_package_count"] != len(dependencies):
            raise ValueError("ZIP release metadata does not match the dependency inventory")
        for dependency in dependencies:
            if not dependency["license_expression"] or not dependency["license_files"]:
                raise ValueError("ZIP dependency inventory is missing licensing information")
            for notice in dependency["license_files"]:
                if notice["path"] not in checksums or checksums[notice["path"]] != notice["sha256"] or not archive.read(notice["path"]):
                    raise ValueError(f"ZIP dependency notice is missing or mismatched: {dependency['name']}")
    checksum = digest(path.read_bytes())
    sidecar = path.with_suffix(path.suffix + ".sha256")
    if check_sidecar and sidecar.is_file():
        if sidecar.read_text(encoding="ascii") != f"{checksum}  {path.name}\n":
            raise ValueError("ZIP SHA-256 sidecar does not match the archive")
    return {"archive": str(path.resolve()), "sha256": checksum, "files": len(names), "dependencies": len(dependencies)}


def write_archive(payload: dict[str, bytes], destination: Path) -> dict:
    payload = dict(payload)
    payload["SHA256SUMS"] = "".join(f"{digest(data)}  {name}\n" for name, data in sorted(payload.items())).encode("utf-8")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=destination.parent, suffix=".tmp", delete=False) as temporary:
        temporary_path = Path(temporary.name)
    try:
        with zipfile.ZipFile(temporary_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name, data in sorted(payload.items()):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.create_system = 0
                info.external_attr = 0o100644 << 16
                archive.writestr(info, data, compresslevel=9)
        result = verify_package(temporary_path, check_sidecar=False)
        os.replace(temporary_path, destination)
    finally:
        temporary_path.unlink(missing_ok=True)
    destination.with_suffix(destination.suffix + ".sha256").write_text(f"{result['sha256']}  {destination.name}\n", encoding="ascii", newline="\n")
    return verify_package(destination)


def package(exe: Path, output_dir: Path) -> dict:
    package_metadata = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]
    data = exe.read_bytes()
    validate_exe(data)
    payload = {"pasteleaf.exe": data}
    for name in ("LICENSE", "README.md", "THIRD_PARTY_NOTICES.md"):
        payload[name] = (ROOT / name).read_bytes()
    metadata = json.loads(run([executable("cargo"), "metadata", "--offline", "--locked", "--filter-platform", TARGET, "--format-version", "1"]))
    inventory = dependency_notices(metadata, payload)
    payload["third-party/DEPENDENCIES.json"] = json_bytes(inventory)
    compiler = rust_notices(payload)
    payload["RELEASE.json"] = json_bytes({
        "name": package_metadata["name"],
        "version": package_metadata["version"],
        "target": TARGET,
        "rustc": compiler,
        "cargo_lock_sha256": digest((ROOT / "Cargo.lock").read_bytes()),
        "third_party_package_count": len(inventory),
    })
    return write_archive(payload, output_dir / f"pasteleaf-{package_metadata['version']}-windows-x64.zip")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, default=ROOT / "target" / "release" / "pasteleaf.exe")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "dist")
    parser.add_argument("--verify", type=Path, help="verify an existing ZIP without rebuilding or resolving dependencies")
    args = parser.parse_args()
    try:
        result = verify_package(args.verify) if args.verify else package(args.exe.resolve(), args.output_dir.resolve())
        print(json.dumps(result, indent=2))
        return 0
    except (OSError, ValueError, KeyError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"Packaging failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
