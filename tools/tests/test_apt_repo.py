"""Checks tools/apt-repo.sh: byte-stable indexes and hashes that match the packages they describe."""

import gzip
import hashlib
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools" / "apt-repo.sh"
EPOCH = "1700000000"


def build_deb(directory: Path) -> Path:
    stage = directory / "stage"
    (stage / "DEBIAN").mkdir(parents=True)
    (stage / "usr" / "bin").mkdir(parents=True)
    (stage / "usr" / "bin" / "stalker-save").write_bytes(b"#!/bin/sh\necho test\n")
    (stage / "DEBIAN" / "control").write_text(
        "Package: stalker-save-editor\nVersion: 2.0.0\nArchitecture: amd64\n"
        "Maintainer: Test <test@example.invalid>\nDescription: test package\n"
    )
    deb = directory / "stalker-save-editor_2.0.0_amd64.deb"
    subprocess.run(["dpkg-deb", "--root-owner-group", "--build", str(stage), str(deb)], check=True, capture_output=True)
    return deb


def run_script(deb_dir: Path, repo_dir: Path) -> None:
    env = dict(os.environ, SOURCE_DATE_EPOCH=EPOCH)
    subprocess.run(["bash", str(SCRIPT), str(deb_dir), str(repo_dir)], check=True, env=env, capture_output=True)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


@unittest.skipUnless(shutil.which("dpkg-deb"), "dpkg-deb is required")
class AptRepoTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="apt-repo-test-"))
        self.deb_dir = self.tmp / "debs"
        self.deb_dir.mkdir()
        self.deb = build_deb(self.tmp)
        shutil.copy(self.deb, self.deb_dir / self.deb.name)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp, ignore_errors=True)

    def test_repeated_runs_produce_identical_bytes(self) -> None:
        first = self.tmp / "first"
        second = self.tmp / "second"
        run_script(self.deb_dir, first)
        run_script(self.deb_dir, second)
        for rel in [
            "dists/stable/main/binary-amd64/Packages",
            "dists/stable/main/binary-amd64/Packages.gz",
            "dists/stable/Release",
        ]:
            self.assertEqual((first / rel).read_bytes(), (second / rel).read_bytes(), rel)

    def test_packages_entry_matches_the_real_deb(self) -> None:
        repo = self.tmp / "repo"
        run_script(self.deb_dir, repo)
        packages = (repo / "dists/stable/main/binary-amd64/Packages").read_text()
        stanza = next(block for block in packages.split("\n\n") if "Package: stalker-save-editor" in block)
        fields = dict(line.split(": ", 1) for line in stanza.splitlines() if ": " in line)
        pool_deb = repo / fields["Filename"]
        self.assertTrue(pool_deb.is_file())
        self.assertEqual(fields["SHA256"], sha256(self.deb))
        self.assertEqual(int(fields["Size"]), self.deb.stat().st_size)

    def test_gzip_index_decompresses_to_the_plain_index(self) -> None:
        repo = self.tmp / "repo"
        run_script(self.deb_dir, repo)
        plain = (repo / "dists/stable/main/binary-amd64/Packages").read_bytes()
        with gzip.open(repo / "dists/stable/main/binary-amd64/Packages.gz", "rb") as handle:
            self.assertEqual(handle.read(), plain)

    def test_release_hashes_match_the_index_files(self) -> None:
        repo = self.tmp / "repo"
        run_script(self.deb_dir, repo)
        release = (repo / "dists/stable/Release").read_text()
        sha_section = release.split("SHA256:\n", 1)[1]
        entries = [line.split() for line in sha_section.splitlines() if line.startswith(" ")]
        self.assertEqual({entry[2] for entry in entries}, {"main/binary-amd64/Packages", "main/binary-amd64/Packages.gz"})
        for digest, size, rel in entries:
            path = repo / "dists/stable" / rel
            self.assertEqual(digest, sha256(path), rel)
            self.assertEqual(int(size), path.stat().st_size, rel)

    def test_missing_epoch_is_refused(self) -> None:
        env = {k: v for k, v in os.environ.items() if k != "SOURCE_DATE_EPOCH"}
        result = subprocess.run(
            ["bash", str(SCRIPT), str(self.deb_dir), str(self.tmp / "repo")], env=env, capture_output=True
        )
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
