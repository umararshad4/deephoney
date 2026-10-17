#!/usr/bin/env python3
"""Build, verify and safely install a local DeepHoney macOS bundle (Python 3.11+)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import shlex
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
NAME = "DeepHoney.app"
IDENTIFIER = "com.deephoney.app"


def run(*args):
    subprocess.run([str(x) for x in args], check=True)


def capture(*args, timeout=60):
    return subprocess.run([str(x) for x in args], check=True, capture_output=True,
                          text=True, timeout=timeout).stdout


def signing_preflight(identity, profile):
    """Only an explicitly named Developer ID identity may leave local ad-hoc mode."""
    if profile and not identity:
        raise ValueError("--notarize-profile requires --sign-identity")
    if not identity:
        return None
    match = re.fullmatch(r"Developer ID Application: [^\r\n]+ \(([A-Z0-9]{10})\)", identity)
    if not match:
        raise ValueError("use the full Developer ID Application identity, including its Team ID")
    identities = capture("/usr/bin/security", "find-identity", "-v", "-p", "codesigning")
    if f'"{identity}"' not in identities:
        raise ValueError("Developer ID identity is not available with a valid signing certificate")
    if profile:
        if not profile.strip() or profile.startswith("-") or any(ord(c) < 32 for c in profile):
            raise ValueError("invalid Keychain profile name")
        for tool in ("notarytool", "stapler"):
            capture("/usr/bin/xcrun", "--find", tool)
    return match.group(1)


def verify_distribution(bundle, team, notarized):
    # Bind verification to Apple's Developer ID certificate chain and expected team;
    # neither an editable manifest nor a self-signed identity proves a publisher.
    if not isinstance(team, str) or not re.fullmatch(r"[A-Z0-9]{10}", team):
        raise ValueError("invalid signing team")
    requirement = (f'anchor apple generic and identifier "{IDENTIFIER}" and '
                   'certificate 1[field.1.2.840.113635.100.6.2.6] exists and '
                   'certificate leaf[field.1.2.840.113635.100.6.1.13] exists and '
                   f'certificate leaf[subject.OU] = "{team}"')
    run("/usr/bin/codesign", "--verify", "--deep", "--strict", "-R", requirement, bundle)
    details = subprocess.run(["/usr/bin/codesign", "--display", "--verbose=4", str(bundle)],
                             check=True, capture_output=True, text=True, timeout=60).stderr
    if not re.search(r"^CodeDirectory .*flags=.*\bruntime\b", details, re.MULTILINE) or not re.search(r"^Timestamp=.+", details, re.MULTILINE):
        raise ValueError("Developer ID signature requires hardened runtime and a secure timestamp")
    if notarized:
        run("/usr/bin/xcrun", "stapler", "validate", bundle)
        run("/usr/sbin/spctl", "--assess", "--type", "execute", "--verbose=2", bundle)


def notarize(bundle, stage, profile):
    # This archive is only for submission. Stapling modifies the bundle, so final
    # hashes and the distributable ZIP must be produced after successful stapling.
    submission = stage / "notary-submission.zip"
    run("/usr/bin/ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", bundle, submission)
    try:
        result = json.loads(capture("/usr/bin/xcrun", "notarytool", "submit", submission,
            "--keychain-profile", profile, "--wait", "--timeout", "20m", "--output-format", "json", timeout=1260))
        if not isinstance(result, dict) or result.get("status") != "Accepted":
            raise ValueError("notarization was not Accepted; inspect the submission in Apple's notary service")
        submission_id = result.get("id")
        if not isinstance(submission_id, str) or not re.fullmatch(r"[0-9a-fA-F-]{36}", submission_id):
            raise ValueError("notarization returned an invalid submission ID")
        run("/usr/bin/xcrun", "stapler", "staple", bundle)
        return submission_id
    finally:
        submission.unlink(missing_ok=True)


def checked_path(raw):
    """Reject symlinks instead of silently redirecting filesystem writes."""
    path = Path(raw).expanduser().absolute()
    for part in [path, *path.parents]:
        if part.is_symlink() and str(part) not in ("/tmp", "/var"):
            raise ValueError(f"symlink path refused: {part}")
    return path.resolve()


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def contents(bundle):
    files = {}
    for path in sorted(bundle.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"bundle symlink refused: {path}")
        if path.is_file():
            files[path.relative_to(bundle).as_posix()] = digest(path)
    return files


def verify(bundle, manifest, require_notarized=False):
    if not bundle.is_dir() or bundle.name != NAME:
        raise ValueError(f"expected {NAME} directory")
    record = json.loads(manifest.read_text())
    if not isinstance(record, dict) or record.get("schema") != 1 or record.get("bundle") != NAME:
        raise ValueError("unsupported manifest")
    if not isinstance(record.get("version"), str) or not isinstance(record.get("files"), dict):
        raise ValueError("invalid manifest fields")
    if contents(bundle) != record.get("files"):
        raise ValueError("bundle checksum mismatch or unexpected files")
    with (bundle / "Contents/Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    if info.get("CFBundleIdentifier") != IDENTIFIER or info.get("DeepHoneyVersion") != record.get("version") or info.get("CFBundleShortVersionString") != record.get("version", "").split("-")[0]:
        raise ValueError("bundle metadata mismatch")
    license_path = bundle / "Contents/Resources/LICENSE"
    if not license_path.is_file() or not license_path.read_text().strip():
        raise ValueError("project license notice missing or empty")
    executable = bundle / "Contents/MacOS/deephoney"
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise ValueError("app executable missing or not executable")
    signing = record.get("signing")
    if require_notarized and signing != "Developer ID; notarized and stapled":
        raise ValueError("a notarized and stapled Developer ID build is required")
    if signing == "ad-hoc; not notarized":
        run("/usr/bin/codesign", "--verify", "--deep", "--strict", bundle)
    elif signing in ("Developer ID; not notarized", "Developer ID; notarized and stapled"):
        verify_distribution(bundle, record.get("team_id"), signing == "Developer ID; notarized and stapled")
    else:
        raise ValueError("unsupported signing status")
    return record


def package(args):
    identity = getattr(args, "sign_identity", None)
    profile = getattr(args, "notarize_profile", None)
    team = signing_preflight(identity, profile)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    output = checked_path(args.output)
    if output.exists():
        raise ValueError(f"output exists; choose a new directory: {output}")
    if args.binary:
        binary = checked_path(args.binary)
    else:
        run("cargo", "build", "--release", "--locked", "--package", "app", "--bin", "deephoney", "--manifest-path", ROOT / "Cargo.toml")
        binary = ROOT / "target/release/deephoney"
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError(f"executable missing: {binary}")
    output.parent.mkdir(parents=True, exist_ok=True)
    # Work beside the destination, then rename atomically. Never overwrite an artifact.
    with tempfile.TemporaryDirectory(prefix=".deephoney-package-", dir=output.parent) as temporary:
        stage = Path(temporary)
        bundle = stage / NAME
        (bundle / "Contents/MacOS").mkdir(parents=True)
        (bundle / "Contents/Resources").mkdir()
        shutil.copyfile(ROOT / "LICENSE", bundle / "Contents/Resources/LICENSE")
        shutil.copyfile(binary, bundle / "Contents/MacOS/deephoney")
        (bundle / "Contents/MacOS/deephoney").chmod(0o755)
        info = {"CFBundleName": "DeepHoney", "CFBundleDisplayName": "DeepHoney",
                "CFBundleIdentifier": IDENTIFIER, "CFBundleExecutable": "deephoney",
                "CFBundlePackageType": "APPL", "CFBundleShortVersionString": version.split("-")[0], "DeepHoneyVersion": version,
                "CFBundleVersion": version.split("-")[0], "LSMinimumSystemVersion": "15.0",
                "NSHighResolutionCapable": True, "NSPrincipalClass": "NSApplication"}
        with (bundle / "Contents/Info.plist").open("wb") as stream:
            plistlib.dump(info, stream)
        signing = "ad-hoc; not notarized"
        submission_id = None
        if identity:
            # Sign the nested executable before the enclosing bundle, without
            # --deep signing or broad entitlements that disable runtime checks.
            for item in (bundle / "Contents/MacOS/deephoney", bundle):
                run("/usr/bin/codesign", "--force", "--options", "runtime", "--timestamp", "--sign", identity, item)
            verify_distribution(bundle, team, False)
            signing = "Developer ID; not notarized"
            if profile:
                submission_id = notarize(bundle, stage, profile)
                verify_distribution(bundle, team, True)
                signing = "Developer ID; notarized and stapled"
        else:
            run("/usr/bin/codesign", "--force", "--sign", "-", bundle)
        manifest = stage / "DeepHoney.manifest.json"
        manifest.write_text(json.dumps({"schema": 1, "bundle": NAME, "version": version,
            "binary_format": subprocess.check_output(["/usr/bin/file", "-b", str(binary)], text=True).strip(),
            "signing": signing, "team_id": team, "notarization_id": submission_id, "files": contents(bundle)}, indent=2) + "\n")
        verify(bundle, manifest)
        archive = stage / f"DeepHoney-{version}-macOS.zip"
        run("/usr/bin/ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", bundle, archive)
        (stage / "SHA256SUMS").write_text(f"{digest(archive)}  {archive.name}\n{digest(manifest)}  {manifest.name}\n")
        # mkdir is exclusive, unlike rename onto an existing empty directory.
        output.mkdir()
        try:
            for item in stage.iterdir():
                shutil.move(str(item), output / item.name)
        except BaseException:
            shutil.rmtree(output)
            raise
    print(f"Packaged {output / NAME}\n{signing}. SHA256SUMS is integrity metadata, not publisher authentication.")


def install(args):
    bundle = checked_path(args.bundle)
    manifest = checked_path(args.manifest) if args.manifest else bundle.parent / "DeepHoney.manifest.json"
    verify(bundle, manifest, getattr(args, "require_notarized", False))
    destination = checked_path(args.destination)
    if destination in (Path("/"), Path("/Applications")) or any(destination.is_relative_to(Path(x)) for x in ("/Applications", "/System", "/usr", "/bin", "/sbin", "/Library", "/private/etc", "/dev")):
        raise ValueError("system destination refused; use ~/Applications or a custom user directory")
    destination.mkdir(parents=True, exist_ok=True)
    target = destination / NAME
    # Exclusive reservation prevents both normal overwrite and dangling-symlink overwrite.
    target.mkdir()
    try:
        # ditto preserves macOS resource forks and stapled-ticket metadata.
        run("/usr/bin/ditto", bundle, target)
        verify(target, manifest, getattr(args, "require_notarized", False))
    except BaseException:
        shutil.rmtree(target)
        raise
    print(f"Installed {target}\nExisting sessions, settings and Keychain were not changed. Open with: open {shlex.quote(str(target))}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("package", help="release build by default, or package an explicit existing executable")
    build.add_argument("--binary", help="existing executable (for development/testing; no source freshness guarantee)")
    build.add_argument("--sign-identity", help="full installed Developer ID Application identity; enables hardened runtime signing")
    build.add_argument("--notarize-profile", help="explicitly upload to Apple using this existing notarytool Keychain profile; requires --sign-identity")
    build.add_argument("--output", required=True, help="new artifact directory; must not exist")
    add = commands.add_parser("install", help="verified copy; refuses any existing DeepHoney.app")
    add.add_argument("bundle")
    add.add_argument("--manifest")
    add.add_argument("--destination", default=str(Path.home() / "Applications"))
    check = commands.add_parser("verify", help="verify bundle hashes, metadata and claimed signature/notarization")
    check.add_argument("bundle")
    check.add_argument("--manifest")
    for command in (check, add):
        command.add_argument("--require-notarized", action="store_true", help="refuse ad-hoc and unnotarized builds; enforce Apple signature, ticket and Gatekeeper checks")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("macOS is required")
    try:
        if args.command == "package":
            package(args)
        elif args.command == "install":
            install(args)
        else:
            bundle = checked_path(args.bundle)
            print(json.dumps(verify(bundle, checked_path(args.manifest) if args.manifest else bundle.parent / "DeepHoney.manifest.json", args.require_notarized), indent=2))
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Error: {error}\n")


if __name__ == "__main__":
    main()
