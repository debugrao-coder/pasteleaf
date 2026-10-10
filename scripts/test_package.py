"""Regression checks for portable-package integrity, without running the app."""

import io
import json
import os
from pathlib import Path
import struct
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package


def pe_image():
    data = bytearray(128)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<H", data, 68, 0x8664)
    return bytes(data)


def payload():
    notice = b"Example dependency license\n"
    member = "third-party/example-1.0.0/LICENSE"
    files = {
        "pasteleaf.exe": pe_image(),
        "LICENSE": b"Project license\n",
        "README.md": b"Usage\n",
        "THIRD_PARTY_NOTICES.md": b"Attributions\n",
        "RELEASE.json": package.json_bytes({"target": package.TARGET, "third_party_package_count": 1}),
        "third-party/rust/COPYRIGHT-library.html": b"Rust copyright\n",
        "third-party/rust/licenses/MIT.txt": b"MIT\n",
        "third-party/rust/licenses/Apache-2.0.txt": b"Apache\n",
        member: notice,
    }
    files["third-party/DEPENDENCIES.json"] = package.json_bytes([{
        "name": "example",
        "license_expression": "MIT",
        "license_files": [{"path": member, "sha256": package.digest(notice)}],
    }])
    return files


class PackageTests(unittest.TestCase):
    def test_only_windows_resolve_graph_is_packaged(self):
        metadata = {
            "resolve": {"root": "root", "nodes": [
                {"id": "root", "dependencies": ["windows"]},
                {"id": "windows", "dependencies": ["build"]},
                {"id": "build", "dependencies": []},
                {"id": "other-os", "dependencies": []},
            ]},
            "packages": [{"id": name, "name": name, "version": "1"} for name in ("root", "windows", "build", "other-os")],
        }
        self.assertEqual([item["name"] for item in package.windows_packages(metadata)], ["build", "windows"])

    def test_cargo_home_and_home_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            # Cargo normalizes Windows 8.3 aliases before locating its cache.
            home = Path(directory).resolve()
            with patch.dict(os.environ, {"HOME": directory}, clear=True):
                self.assertEqual(package.cargo_home(), home / ".cargo")
            with patch.dict(os.environ, {"HOME": directory, "CARGO_HOME": str(home / "cache")}, clear=True):
                self.assertEqual(package.cargo_home(), home / "cache")

    def test_recursive_license_names_and_declared_path_boundaries(self):
        with tempfile.TemporaryDirectory() as directory:
            crate = Path(directory) / "crate"
            (crate / "tests").mkdir(parents=True)
            (crate / "license-mit").write_text("MIT")
            (crate / "tests" / "COPYRIGHT").write_text("Copyright")
            (crate / "src.rs").write_text("// code")
            self.assertEqual([file.relative_to(crate).as_posix() for file in package.notice_files(crate, None)], ["license-mit", "tests/COPYRIGHT"])
            with self.assertRaisesRegex(ValueError, "outside"):
                package.notice_files(crate, "../LICENSE")

    def test_archive_is_complete_and_repeatable_for_identical_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first" / "test.zip"
            second = Path(directory) / "second" / "test.zip"
            result = package.write_archive(payload(), first)
            self.assertEqual(result["dependencies"], 1)
            self.assertEqual(result["files"], 11)
            self.assertEqual(result["sha256"], package.write_archive(payload(), second)["sha256"])
            self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_corrupt_payload_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "test.zip"
            package.write_archive(payload(), archive)
            with zipfile.ZipFile(archive) as source:
                files = {name: source.read(name) for name in source.namelist()}
            files["README.md"] += b"tampered"
            with zipfile.ZipFile(archive, "w") as destination:
                for name, data in files.items():
                    destination.writestr(name, data)
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                package.verify_package(archive)

    def test_missing_license_inventory_file_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            files = payload()
            del files["third-party/example-1.0.0/LICENSE"]
            with self.assertRaisesRegex(ValueError, "notice is missing"):
                package.write_archive(files, Path(directory) / "test.zip")

    def test_unsafe_path_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            files = payload()
            files["../library.json"] = b"private data"
            with self.assertRaisesRegex(ValueError, "unsafe"):
                package.write_archive(files, Path(directory) / "test.zip")

    def test_external_hash_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "test.zip"
            package.write_archive(payload(), archive)
            archive.with_suffix(".zip.sha256").write_text("0" * 64 + "  test.zip\n", encoding="ascii")
            with self.assertRaisesRegex(ValueError, "sidecar"):
                package.verify_package(archive)

    def test_boost_fallback_is_pinned_and_hash_checked(self):
        source = "registry+https://github.com/rust-lang/crates.io-index"
        package_id = "clipboard-win"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cache = root / "cargo"
            crate = cache / "registry" / "src" / "index" / "clipboard-win-5.4.1"
            crate.mkdir(parents=True)
            archive = cache / "registry" / "cache" / "index" / "clipboard-win-5.4.1.crate"
            archive.parent.mkdir(parents=True)
            with tarfile.open(archive, "w:gz") as bundle:
                data = b'license = "BSL-1.0"\n'
                member = tarfile.TarInfo("clipboard-win-5.4.1/Cargo.toml")
                member.size = len(data)
                bundle.addfile(member, io.BytesIO(data))
            checksum = package.digest(archive.read_bytes())
            (root / "Cargo.lock").write_text(f'[[package]]\nname = "clipboard-win"\nversion = "5.4.1"\nsource = "{source}"\nchecksum = "{checksum}"\n')
            assets = root / "license_assets"
            assets.mkdir()
            license_data = b"Boost license fixture\n"
            (assets / "BSL-1.0.txt").write_bytes(license_data)
            (assets / "SOURCES.json").write_text(json.dumps({"BSL-1.0.txt": {
                "license": "BSL-1.0", "used_by": [{"name": "clipboard-win", "version": "5.4.1"}],
                "sha256": package.digest(license_data), "source": "https://www.boost.org/LICENSE_1_0.txt", "reason": "fixture",
            }}))
            metadata = {"resolve": {"root": "root", "nodes": [
                {"id": "root", "dependencies": [package_id]}, {"id": package_id, "dependencies": []},
            ]}, "packages": [{"id": package_id, "name": "clipboard-win", "version": "5.4.1", "source": source,
                               "license": "BSL-1.0", "manifest_path": str(crate / "Cargo.toml"), "repository": None}]}
            with patch.object(package, "ROOT", root), patch.dict(os.environ, {"CARGO_HOME": str(cache)}):
                inventory = package.dependency_notices(metadata, {})
                self.assertEqual(inventory[0]["license_files"][0]["origin"], "vendored_official_license")
                (assets / "BSL-1.0.txt").write_bytes(license_data + b"tampered")
                with self.assertRaisesRegex(ValueError, "Vendored license hash"):
                    package.dependency_notices(metadata, {})


if __name__ == "__main__":
    unittest.main()
