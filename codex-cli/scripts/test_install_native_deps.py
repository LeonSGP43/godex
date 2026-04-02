#!/usr/bin/env python3

import io
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import install_native_deps


class ExtractArchiveTest(unittest.TestCase):
    def test_extract_archive_tar_gz_copies_regular_file(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            tempdir_path = Path(tempdir)
            archive_path = tempdir_path / "rg.tar.gz"
            dest = tempdir_path / "out" / "rg"
            payload = b"#!/bin/sh\necho rg\n"

            with tarfile.open(archive_path, "w:gz") as tar:
                info = tarfile.TarInfo("ripgrep-15.1.0-aarch64-apple-darwin/rg")
                info.size = len(payload)
                tar.addfile(info, io.BytesIO(payload))

            install_native_deps.extract_archive(
                archive_path,
                "tar.gz",
                "ripgrep-15.1.0-aarch64-apple-darwin/rg",
                dest,
            )

            self.assertEqual(dest.read_bytes(), payload)

    def test_extract_archive_tar_gz_rejects_non_regular_file(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            tempdir_path = Path(tempdir)
            archive_path = tempdir_path / "rg.tar.gz"
            dest = tempdir_path / "out" / "rg"

            with tarfile.open(archive_path, "w:gz") as tar:
                info = tarfile.TarInfo("ripgrep-15.1.0-aarch64-apple-darwin/rg")
                info.type = tarfile.SYMTYPE
                info.linkname = "elsewhere"
                tar.addfile(info)

            with self.assertRaisesRegex(RuntimeError, "not a regular file"):
                install_native_deps.extract_archive(
                    archive_path,
                    "tar.gz",
                    "ripgrep-15.1.0-aarch64-apple-darwin/rg",
                    dest,
                )


if __name__ == "__main__":
    unittest.main()
