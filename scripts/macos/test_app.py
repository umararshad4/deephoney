"""Focused filesystem/signing checks; no application or user profile is launched."""
import importlib.util
import json
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

SCRIPT = Path(__file__).with_name("app.py")
spec = importlib.util.spec_from_file_location("packager", SCRIPT)
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)


@unittest.skipUnless(sys.platform == "darwin", "macOS signing tools required")
class PackagingTests(unittest.TestCase):
    def setUp(self):
        scratch = packager.ROOT / ".scratch"
        scratch.mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="installation test ", dir=scratch)
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def command(self, *args, success=True):
        result = subprocess.run([sys.executable, str(SCRIPT), *map(str, args)], capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result

    def package(self):
        output = self.root / "artifact with spaces"
        self.command("package", "--binary", "/usr/bin/true", "--output", output)
        return output

    def test_package_install_verify_no_overwrite_and_preserved_data(self):
        output = self.package()
        self.command("verify", output / "DeepHoney.app")
        refused = self.command("verify", output / "DeepHoney.app", "--require-notarized", success=False)
        self.assertIn("notarized and stapled Developer ID build is required", refused.stderr)
        self.command("install", output / "DeepHoney.app", "--require-notarized",
                     "--destination", self.root / "distribution only", success=False)
        self.assertFalse((self.root / "distribution only").exists())
        record = json.loads((output / "DeepHoney.manifest.json").read_text())
        license_path = output / "DeepHoney.app/Contents/Resources/LICENSE"
        self.assertEqual(license_path.read_bytes(), (packager.ROOT / "LICENSE").read_bytes())
        self.assertIn("Contents/Resources/LICENSE", record["files"])
        self.assertEqual(record["version"], packager.tomllib.loads((packager.ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"])
        with (output / "DeepHoney.app/Contents/Info.plist").open("rb") as stream:
            self.assertEqual(plistlib.load(stream)["CFBundleExecutable"], "deephoney")
        archives = list(output.glob("*.zip"))
        self.assertEqual(len(archives), 1)
        extracted = self.root / "unpacked zip"
        subprocess.run(["/usr/bin/ditto", "-x", "-k", str(archives[0]), str(extracted)], check=True)
        self.command("verify", extracted / "DeepHoney.app", "--manifest", output / "DeepHoney.manifest.json")
        destination = self.root / "user apps"
        destination.mkdir()
        preserved = destination / "sessions.db"
        preserved.write_bytes(b"untouched")
        self.command("install", output / "DeepHoney.app", "--destination", destination)
        self.command("install", output / "DeepHoney.app", "--destination", destination, success=False)
        self.assertEqual(preserved.read_bytes(), b"untouched")
        self.command("package", "--binary", "/usr/bin/true", "--output", output, success=False)

    def test_symlinks_bad_destinations_and_tampering_refused(self):
        output = self.package()
        target = self.root / "real directory"
        target.mkdir()
        link = self.root / "linked directory"
        link.symlink_to(target, target_is_directory=True)
        self.command("install", output / "DeepHoney.app", "--destination", link, success=False)
        self.command("package", "--binary", "/usr/bin/true", "--output", link / "new", success=False)
        for destination in ("/", "/Applications", "/Applications/Research Tools", "/System/DeepHoney-test", "/usr/local/DeepHoney-test"):
            self.command("install", output / "DeepHoney.app", "--destination", destination, success=False)
        dangling = target / "DeepHoney.app"
        dangling.symlink_to(target / "missing")
        self.command("install", output / "DeepHoney.app", "--destination", target, success=False)
        executable = output / "DeepHoney.app/Contents/MacOS/deephoney"
        executable.chmod(0o644)
        self.command("verify", output / "DeepHoney.app", success=False)
        executable.chmod(0o755)
        (output / "DeepHoney.app/Contents/Resources/extra").write_text("unexpected")
        self.command("verify", output / "DeepHoney.app", success=False)
        self.command("install", output / "DeepHoney.app", "--destination", self.root / "other", success=False)
        self.assertFalse((self.root / "other").exists())
        # Even a self-consistent manifest/signature must not permit omitting the MIT notice.
        extra = output / "DeepHoney.app/Contents/Resources/extra"
        extra.unlink()
        (output / "DeepHoney.app/Contents/Resources/LICENSE").unlink()
        subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", str(output / "DeepHoney.app")], check=True, capture_output=True)
        manifest_path = output / "DeepHoney.manifest.json"
        record = json.loads(manifest_path.read_text())
        record["files"] = packager.contents(output / "DeepHoney.app")
        manifest_path.write_text(json.dumps(record))
        refused = self.command("verify", output / "DeepHoney.app", success=False)
        self.assertIn("license notice missing", refused.stderr)


class DistributionTests(unittest.TestCase):
    identity = "Developer ID Application: Example Publisher (ABCDE12345)"

    def test_preflight_requires_explicit_valid_installed_identity(self):
        with patch.object(packager, "capture") as capture:
            self.assertIsNone(packager.signing_preflight(None, None))
            capture.assert_not_called()
            for identity, profile in ((None, "profile"), ("-", None),
                                      ("Apple Development: Example (ABCDE12345)", None)):
                with self.assertRaises(ValueError):
                    packager.signing_preflight(identity, profile)
            capture.return_value = "0 valid identities found"
            with self.assertRaisesRegex(ValueError, "not available"):
                packager.signing_preflight(self.identity, None)
            capture.return_value = f'1) ABCD "{self.identity}"'
            self.assertEqual(packager.signing_preflight(self.identity, "release profile"), "ABCDE12345")
            self.assertIn(("/usr/bin/xcrun", "--find", "notarytool"), [call.args for call in capture.call_args_list])
            for bad in (" ", "-unexpected", "line\nbreak"):
                with self.assertRaises(ValueError):
                    packager.signing_preflight(self.identity, bad)

    def test_notarization_rejection_timeout_and_staple_failure_do_not_succeed(self):
        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary)
            bundle = stage / packager.NAME
            bundle.mkdir()
            def archive(*args):
                if args[0] == "/usr/bin/ditto":
                    Path(args[-1]).write_bytes(b"submission")
            for response in ('{"status":"Invalid"}', '{"status":"Accepted","id":"wrong"}', 'not json'):
                with patch.object(packager, "run", side_effect=archive) as run, patch.object(packager, "capture", return_value=response):
                    with self.assertRaises(ValueError):
                        packager.notarize(bundle, stage, "profile")
                    self.assertFalse(any("staple" in call.args for call in run.call_args_list))
                    self.assertFalse((stage / "notary-submission.zip").exists())
            with patch.object(packager, "run", side_effect=archive), patch.object(packager, "capture", side_effect=subprocess.TimeoutExpired("notarytool", 1260)):
                with self.assertRaises(subprocess.TimeoutExpired):
                    packager.notarize(bundle, stage, "profile")
                self.assertFalse((stage / "notary-submission.zip").exists())
            submission_id = "12345678-1234-1234-1234-123456789abc"
            def fail_staple(*args):
                archive(*args)
                if "staple" in args:
                    raise subprocess.CalledProcessError(1, "stapler")
            with patch.object(packager, "run", side_effect=fail_staple) as run, patch.object(packager, "capture", return_value=json.dumps({"status":"Accepted", "id":submission_id})) as capture:
                with self.assertRaises(subprocess.CalledProcessError):
                    packager.notarize(bundle, stage, "profile")
                self.assertIn(("/usr/bin/xcrun", "stapler", "staple", bundle), [call.args for call in run.call_args_list])
                self.assertIn("--wait", capture.call_args.args)
                self.assertEqual(capture.call_args.kwargs["timeout"], 1260)
                self.assertFalse((stage / "notary-submission.zip").exists())

    def test_distribution_verification_checks_chain_runtime_timestamp_and_ticket(self):
        bundle = Path("DeepHoney.app")
        with patch.object(packager, "run") as run, patch.object(packager.subprocess, "run") as details:
            details.return_value = SimpleNamespace(stderr="CodeDirectory v=20500 flags=0x10000(runtime) hashes=2\nTimestamp=Oct 18, 2026\n")
            packager.verify_distribution(bundle, "ABCDE12345", True)
            commands = [call.args for call in run.call_args_list]
            self.assertIn('certificate leaf[subject.OU] = "ABCDE12345"', commands[0][-2])
            self.assertIn('certificate leaf[field.1.2.840.113635.100.6.1.13] exists', commands[0][-2])
            self.assertIn(("/usr/bin/xcrun", "stapler", "validate", bundle), commands)
            self.assertIn(("/usr/sbin/spctl", "--assess", "--type", "execute", "--verbose=2", bundle), commands)
            for invalid in ("CodeDirectory flags=0x0(none)\nTimestamp=today", "CodeDirectory flags=0x10000(runtime)\nSigned Time=today"):
                details.return_value = SimpleNamespace(stderr=invalid)
                with self.assertRaisesRegex(ValueError, "runtime and a secure timestamp"):
                    packager.verify_distribution(bundle, "ABCDE12345", False)
            with self.assertRaises(ValueError):
                packager.verify_distribution(bundle, 'malicious" requirement', True)

    def test_package_notarizes_before_final_hashes_and_cleans_failed_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "binary"
            binary.write_bytes(b"fixture")
            binary.chmod(0o755)
            args = SimpleNamespace(binary=str(binary), output=str(root / "accepted"),
                                   sign_identity=self.identity, notarize_profile="profile")
            def command(*items):
                if items[0] == "/usr/bin/ditto":
                    Path(items[-1]).write_bytes(b"archive")
            def staple(bundle, stage, profile):
                (bundle / "Contents/Resources/ticket").write_text("stapled before hashes")
                return "12345678-1234-1234-1234-123456789abc"
            with patch.object(packager, "signing_preflight", return_value="ABCDE12345"), patch.object(packager, "run", side_effect=command) as run, patch.object(packager, "notarize", side_effect=staple), patch.object(packager, "verify_distribution"), patch.object(packager.subprocess, "check_output", return_value="fixture"):
                packager.package(args)
                record = json.loads((root / "accepted/DeepHoney.manifest.json").read_text())
                self.assertEqual(record["signing"], "Developer ID; notarized and stapled")
                self.assertEqual(record["team_id"], "ABCDE12345")
                self.assertIn("Contents/Resources/ticket", record["files"])
                signing_calls = [c.args for c in run.call_args_list if c.args[0] == "/usr/bin/codesign"]
                self.assertEqual(len(signing_calls), 2)
                self.assertTrue(all("runtime" in call and "--timestamp" in call for call in signing_calls))
            args.output = str(root / "rejected")
            with patch.object(packager, "signing_preflight", return_value="ABCDE12345"), patch.object(packager, "run", side_effect=command), patch.object(packager, "verify_distribution"), patch.object(packager, "notarize", side_effect=ValueError("rejected")):
                with self.assertRaisesRegex(ValueError, "rejected"):
                    packager.package(args)
                self.assertFalse((root / "rejected").exists())
                self.assertFalse(list(root.glob(".deephoney-package-*")))


if __name__ == "__main__":
    unittest.main()
