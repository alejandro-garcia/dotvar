#!/usr/bin/env python3
"""Package Dotvar for local use or direct distribution on Apple Silicon."""

import argparse
import fcntl
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import tempfile


BUNDLE_ID = "com.dotvar.app"
APP_NAME = "Dotvar.app"
TARGET = "aarch64-apple-darwin"
MINIMUM_MACOS = "15.0"


def distribution_credentials(root):
    config = root / "local" / "release.json"
    settings = json.loads(config.read_text()) if config.is_file() else {}
    profile = os.environ.get("NOTARY_PROFILE") or settings.get("notary_profile")
    if not isinstance(profile, str) or not profile.strip():
        raise SystemExit("Set NOTARY_PROFILE or notary_profile in local/release.json.")

    listing = subprocess.check_output(
        ["security", "find-identity", "-v", "-p", "codesigning"], text=True
    )
    identities = set(re.findall(r'\b([0-9A-Fa-f]{40})\s+"Developer ID Application:[^"]+"', listing))
    requested_identity = os.environ.get("SIGN_IDENTITY")
    if requested_identity:
        identity = requested_identity.upper()
        if identity not in identities:
            raise SystemExit("SIGN_IDENTITY must be the SHA-1 of a valid Developer ID Application identity.")
    elif len(identities) == 1:
        identity = next(iter(identities))
    else:
        raise SystemExit("Install a Developer ID Application identity or select one with SIGN_IDENTITY.")

    credentials = ["--keychain-profile", profile]
    if keychain := os.environ.get("NOTARY_KEYCHAIN"):
        credentials.extend(["--keychain", keychain])
    result = subprocess.run(["xcrun", "notarytool", "history", *credentials, "--output-format", "json"],
                            stdout=subprocess.DEVNULL)
    if result.returncode:
        raise SystemExit("Could not authenticate with Apple using the notarytool Keychain profile.")
    return identity, credentials


def cargo_metadata(root):
    return json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=root
    ))


def app_version(metadata):
    return next(package["version"] for package in metadata["packages"]
                if package["name"] == "env-desktop")


def build_executable(root, metadata):
    build_directory = Path(metadata["target_directory"])
    if metadata.get("build_directory"):
        build_directory = Path(metadata["build_directory"])
    home = Path.home()
    rustflags = [
        f"--remap-path-prefix={home}=/dotvar-build/home",
        f"--remap-path-prefix={root}=/dotvar-build/source",
        f"--remap-path-prefix={build_directory}=/dotvar-build/target",
    ]
    environment = dict(os.environ, MACOSX_DEPLOYMENT_TARGET=MINIMUM_MACOS,
                       CARGO_ENCODED_RUSTFLAGS="\x1f".join(rustflags))
    subprocess.run(["cargo", "build", "-p", "env-desktop", "--bin", "dotvar",
                    "--release", "--locked", "--target", TARGET], cwd=root,
                   env=environment, check=True)
    return Path(metadata["target_directory"]) / TARGET / "release" / "dotvar"


def create_app(app, executable, icon, version, identity):
    contents = app / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Resources").mkdir()
    shutil.copy2(executable, contents / "MacOS" / "dotvar")
    shutil.copy2(icon, contents / "Resources" / "Dotvar.icns")
    with (contents / "Info.plist").open("wb") as plist:
        plistlib.dump({
            "CFBundleDisplayName": "Dotvar",
            "CFBundleExecutable": "dotvar",
            "CFBundleIconFile": "Dotvar.icns",
            "CFBundleIdentifier": BUNDLE_ID,
            "CFBundleName": "Dotvar",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version,
            "CFBundleVersion": version,
            "LSMinimumSystemVersion": MINIMUM_MACOS,
            "NSHighResolutionCapable": True,
        }, plist)
    signing_options = ["--timestamp", "--options", "runtime"] if identity != "-" else []
    subprocess.run(["codesign", "--force", "--sign", identity, *signing_options, app], check=True)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", app], check=True)


def notarize(dmg, credentials):
    print(f"Submitting {dmg.name} to Apple for notarization.", flush=True)
    result = subprocess.run(
        ["xcrun", "notarytool", "submit", dmg, *credentials, "--wait", "--output-format", "json"],
        stdout=subprocess.PIPE, text=True
    )
    receipt = json.loads(result.stdout or "{}")
    status = receipt.get("status")
    if result.returncode or status != "Accepted":
        if submission_id := receipt.get("id"):
            subprocess.run(["xcrun", "notarytool", "log", submission_id, *credentials], check=False)
        raise SystemExit(f"Apple notarization did not succeed (status: {status or 'unknown'}).")
    print("Apple accepted the notarization submission.", flush=True)
    subprocess.run(["xcrun", "stapler", "staple", dmg], check=True)
    subprocess.run(["xcrun", "stapler", "validate", dmg], check=True)


def verify_dmg(dmg, mountpoint):
    subprocess.run(["codesign", "--verify", "--strict", dmg], check=True)
    subprocess.run(["spctl", "--assess", "--type", "open", "--context", "context:primary-signature",
                    "--verbose=2", dmg], check=True)
    subprocess.run(["hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint", mountpoint, dmg],
                   check=True, stdout=subprocess.DEVNULL)
    try:
        if (mountpoint / "Applications").readlink() != Path("/Applications"):
            raise SystemExit("The DMG is missing the Applications installation link.")
        app = mountpoint / APP_NAME
        subprocess.run(["codesign", "--verify", "--deep", "--strict", app], check=True)
        subprocess.run(["xcrun", "lipo", "-verify_arch", "arm64", app / "Contents/MacOS/dotvar"], check=True)
        subprocess.run(["spctl", "--assess", "--type", "execute", "--verbose=2", app], check=True)
    finally:
        subprocess.run(["hdiutil", "detach", mountpoint], check=True, stdout=subprocess.DEVNULL)


def create_distribution(work, source, version, identity, credentials):
    name = f"Dotvar-{version}-macOS-arm64"
    release = work / name
    release.mkdir()
    dmg = release / f"{name}.dmg"
    (source / "Applications").symlink_to("/Applications")
    subprocess.run(["hdiutil", "create", "-srcfolder", source, "-volname", "Dotvar",
                    "-format", "UDZO", dmg], check=True)
    subprocess.run(["codesign", "--sign", identity, "--timestamp", "--identifier", f"{BUNDLE_ID}.dmg",
                    dmg], check=True)
    notarize(dmg, credentials)
    verify_dmg(dmg, work / "mount")
    checksum = subprocess.check_output(["shasum", "-a", "256", dmg.name], cwd=release, text=True)
    (release / f"{dmg.name}.sha256").write_text(checksum)
    return release


def install_result(staged, output, work):
    destination = output / staged.name
    previous = work / "previous"
    # Serialize replacement so concurrent runs cannot interleave the backup and rename.
    with (output / ".package.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if destination.exists():
            destination.rename(previous)
        try:
            staged.rename(destination)
        except OSError:
            if previous.exists():
                previous.rename(destination)
            raise
    return destination


def require_signing_mac():
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("Run this script on an Apple Silicon Mac.")


def package(root, output, signing=None):
    identity, credentials = signing if signing else ("-", None)
    metadata = cargo_metadata(root)
    version = app_version(metadata)
    executable = build_executable(root, metadata)
    output.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".dotvar-", dir=output) as directory:
        work = Path(directory)
        source = work / "volume"
        app = source / APP_NAME
        create_app(app, executable, root / "assets" / "Dotvar.icns", version, identity)
        result = app
        if credentials:
            result = create_distribution(work, source, version, identity, credentials)
        destination = install_result(result, output, work)
    return destination / f"{destination.name}.dmg" if credentials else destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--distribution", action="store_true", help="sign and notarize an installable DMG")
    args = parser.parse_args()
    require_signing_mac()
    root = Path(__file__).resolve().parent.parent
    signing = distribution_credentials(root) if args.distribution else None
    print(package(root, root / "dist", signing))


if __name__ == "__main__":
    main()
