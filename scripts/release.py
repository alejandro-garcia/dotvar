#!/usr/bin/env python3
"""Build a signed Dotvar release locally and publish it with --publish."""

import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]
REPO = "nicoandrade/dotvar"
DOWNLOAD = f"https://github.com/{REPO}/releases/latest/download"
DMG = "Dotvar.dmg"
CHECKSUM = f"{DMG}.sha256"

spec = importlib.util.spec_from_file_location("package_macos", ROOT / "scripts/package-macos.py")
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def github(path="", *, data=None, method="GET", optional=False, paginate=False):
    endpoint = f"repos/{REPO}{'/' + path if path else ''}"
    args = ["gh", "api", endpoint, "--hostname", "github.com", "--method", method]
    if data is not None:
        args += ["--input", "-"]
    if paginate:
        args += ["--paginate", "--slurp"]
    result = subprocess.run(args, input=json.dumps(data) if data is not None else None,
                            cwd=ROOT, capture_output=True, text=True)
    if optional and result.returncode and "HTTP 404" in result.stderr:
        return None
    require(result.returncode == 0, f"GitHub {method} {path or REPO} failed: {result.stderr.strip()}")
    return json.loads(result.stdout) if result.stdout.strip() else None


def version_tuple(tag):
    require(re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag),
            f"Expected a stable version tag vMAJOR.MINOR.PATCH, got {tag}.")
    return tuple(map(int, tag[1:].split(".")))


def release_for(tag):
    pages = github("releases?per_page=100", paginate=True)
    matches = [release for page in pages for release in page if release["tag_name"] == tag]
    require(len(matches) <= 1, f"Multiple GitHub releases use {tag}. Inspect them before retrying.")
    return matches[0] if matches else None


def require_new_latest(tag):
    latest = github("releases/latest", optional=True)
    require(latest is None or version_tuple(tag) > version_tuple(latest["tag_name"]),
            f"{tag} must be newer than the current Latest release.")


def tag_exists(tag, sha):
    ref = github(f"git/ref/tags/{tag}", optional=True)
    if ref is None:
        return False
    commit = github(f"commits/refs/tags/{tag}") if ref["object"]["type"] == "tag" else ref["object"]
    require(commit["sha"] == sha, f"The existing tag {tag} points to a different commit.")
    return True


def checkout_commit(branch):
    allowed_origins = [f"https://github.com/{REPO}", f"git@github.com:{REPO}",
                       f"ssh://git@github.com/{REPO}"]
    origin = git("remote", "get-url", "origin").rstrip("/")
    require(origin in [url for base in allowed_origins for url in (base, f"{base}.git")],
            f"origin must point to {REPO}.")
    require(git("branch", "--show-current") == branch, f"Run the release from {branch}.")
    require(not git("status", "--porcelain", "--untracked-files=normal"),
            "Commit and push your changes before publishing.")
    sha = git("rev-parse", "HEAD")
    require_branch_head(branch, sha)
    return sha


def require_branch_head(branch, sha):
    head = github(f"git/ref/heads/{branch}")["object"]["sha"]
    require(sha == head, f"The release commit must match GitHub's {branch} branch.")


def wait_for_ci(branch, sha):
    command = [
        "gh", "run", "list", "--repo", f"github.com/{REPO}", "--workflow", "ci.yml", "--branch", branch,
        "--event", "push", "--commit", sha, "--limit", "1", "--json", "databaseId,status,conclusion",
    ]
    for attempt in range(6):
        runs = json.loads(subprocess.check_output(command, cwd=ROOT))
        if runs:
            break
        if attempt < 5:
            time.sleep(3)
    require(runs, "GitHub Actions has not started CI for this commit yet. Retry after the run appears.")
    run = runs[0]
    if run["status"] != "completed":
        print("Waiting for GitHub Actions CI.", flush=True)
        subprocess.run(["gh", "run", "watch", str(run["databaseId"]), "--repo", f"github.com/{REPO}",
                        "--exit-status"], cwd=ROOT, check=True)
    else:
        require(run["conclusion"] == "success", "GitHub Actions CI must pass before publishing.")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def release_files(tag, sha, signing):
    destination = ROOT / "dist" / "releases" / tag / sha
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".release-", dir=destination.parent) as directory:
        work = Path(directory)
        if not destination.exists():
            source = work / "source"
            source.mkdir()
            archive = work / "source.tar"
            subprocess.run(["git", "archive", "--format=tar", f"--output={archive}", sha],
                           cwd=ROOT, check=True)
            subprocess.run(["tar", "-xf", archive, "-C", source], check=True)
            dmgs = work / "packages"
            dmg = packager.package(source, dmgs, signing)
            require(dmg.name == f"Dotvar-{tag[1:]}-macOS-arm64.dmg", "Packaged version does not match the tag.")
            staging = work / "assets"
            staging.mkdir()
            shutil.copyfile(dmg, staging / DMG)
            (staging / CHECKSUM).write_text(f"{sha256(staging / DMG)}  {DMG}\n")
            staging.rename(destination)
        else:
            dmg = destination / DMG
            subprocess.run(["xcrun", "stapler", "validate", dmg], check=True)
            packager.verify_dmg(dmg, work / "mount")
    files = {name: destination / name for name in (DMG, CHECKSUM)}
    require(files[CHECKSUM].read_text() == f"{sha256(files[DMG])}  {DMG}\n", "Local release checksum mismatch.")
    return files


def verify_assets(release, files=None):
    assets = {asset["name"]: asset for asset in release["assets"]}
    require(set(assets) == {DMG, CHECKSUM}, "The release must contain the DMG and its checksum.")
    for name, asset in assets.items():
        require(asset["state"] == "uploaded" and asset["size"] > 0, f"Incomplete upload: {name}.")
        require(re.fullmatch(r"sha256:[a-f0-9]{64}", asset.get("digest") or ""), f"No GitHub digest for {name}.")
        if files is not None:
            path = files[name]
            require(asset["size"] == path.stat().st_size and asset["digest"] == f"sha256:{sha256(path)}",
                    f"Uploaded bytes do not match the local package: {name}.")
    return {name: asset["digest"].split(":", 1)[1] for name, asset in assets.items()}


def release_notes(tag, sha):
    if git("ls-tree", "--name-only", sha, "--", "CHANGELOG.md"):
        changelog = git("show", f"{sha}:CHANGELOG.md")
        heading = rf"^## \[{re.escape(tag[1:])}\](?: - \d{{4}}-\d{{2}}-\d{{2}})?[ \t]*$"
        entry = re.search(heading, changelog, re.MULTILINE)
        if entry:
            notes = re.split(r"^## |^\[[^\n]+\]: ", changelog[entry.end():], maxsplit=1, flags=re.MULTILINE)[0]
            if notes.strip():
                return notes.strip()
    return github("releases/generate-notes", method="POST", data={"tag_name": tag, "target_commitish": sha})["body"]


def publish_draft(tag, sha, branch, files):
    require_branch_head(branch, sha)
    require_new_latest(tag)
    draft = release_for(tag)
    require(draft is None or (draft["draft"] and not draft["prerelease"] and draft["target_commitish"] == sha),
            f"Only a draft for {tag} and this exact commit can be resumed.")
    if not tag_exists(tag, sha):
        github("git/refs", method="POST", data={"ref": f"refs/tags/{tag}", "sha": sha})
    if draft is None:
        body = ("Requires Apple Silicon and macOS 15 or later. Open `Dotvar.dmg` and drag Dotvar to Applications.\n\n"
                "Verify the download with `shasum -a 256 -c Dotvar.dmg.sha256`.\n\n" + release_notes(tag, sha))
        draft = github("releases", method="POST", data={
            "tag_name": tag, "target_commitish": sha, "name": f"Dotvar {tag[1:]}", "body": body,
            "draft": True, "prerelease": False,
        })

    pending = dict(files)
    for asset in draft["assets"]:
        name = asset["name"]
        require(name in pending, f"Unexpected draft asset: {name}.")
        if asset["state"] == "starter" and asset["size"] == 0:
            github(f"releases/assets/{asset['id']}", method="DELETE")
        else:
            path = pending.pop(name)
            require(asset["state"] == "uploaded" and asset["size"] == path.stat().st_size
                    and asset.get("digest") == f"sha256:{sha256(path)}", f"Draft asset differs from the package: {name}.")
    if pending:
        subprocess.run(["gh", "release", "upload", tag, *pending.values(), "--repo", f"github.com/{REPO}"],
                       cwd=ROOT, check=True)

    endpoint = f"releases/{draft['id']}"
    updated = github(endpoint)
    require(updated["draft"] and updated["target_commitish"] == sha, "The draft changed during upload.")
    verify_assets(updated, files)
    require_branch_head(branch, sha)
    require(tag_exists(tag, sha), "The release tag disappeared during upload.")
    require_new_latest(tag)
    release = github(endpoint, method="PATCH", data={"draft": False, "make_latest": "true"})
    require(not release["draft"] and not release["prerelease"], "GitHub did not publish the stable release.")
    return release


def verify_public_download(release, tag):
    print(release["html_url"], flush=True)
    hashes = verify_assets(release)
    for attempt in range(5):
        try:
            latest = github("releases/latest", optional=True)
            require(latest and latest["tag_name"] == tag, f"GitHub Latest has not resolved to {tag}.")
            for name, digest in hashes.items():
                request = Request(f"{DOWNLOAD}/{name}", headers={"User-Agent": "Dotvar-release-check"})
                with urlopen(request, timeout=60) as response:
                    data = response.read()
                require(hashlib.sha256(data).hexdigest() == digest, f"Public download differs: {name}.")
                if name == CHECKSUM:
                    require(data == f"{hashes[DMG]}  {DMG}\n".encode(), "The public checksum does not match the DMG.")
            print(f"Verified public download: {DOWNLOAD}/{DMG}")
            return
        except (OSError, RuntimeError) as error:
            if attempt == 4:
                raise RuntimeError(f"{tag} was published but the public download could not be verified: {error}") from error
            time.sleep(2 ** attempt)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="check GitHub access, version and Apple signing credentials")
    mode.add_argument("--publish", action="store_true", help="publish the verified package from GitHub's default branch")
    args = parser.parse_args()
    packager.require_signing_mac()
    if not (args.check or args.publish):
        print(packager.package(ROOT, ROOT / "dist", packager.distribution_credentials(ROOT)))
        return

    repo = github()
    require(not repo["private"] and repo.get("permissions", {}).get("push"),
            f"Authenticate gh with write access to public repo {REPO}.")
    tag = f"v{packager.app_version(packager.cargo_metadata(ROOT))}"
    version_tuple(tag)
    if args.check:
        packager.distribution_credentials(ROOT)
        print(f"{tag}: GitHub access and Apple signing are ready. Default branch: {repo['default_branch']}.")
        return

    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    with (dist / ".release.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise RuntimeError("Another local Dotvar release is already running.") from error
        branch = repo["default_branch"]
        sha = checkout_commit(branch)
        release = release_for(tag)
        if release and not release["draft"]:
            require(tag_exists(tag, sha), f"Existing release {tag} lacks its tag.")
        else:
            require_new_latest(tag)
            tag_exists(tag, sha)
            signing = packager.distribution_credentials(ROOT)
            wait_for_ci(branch, sha)
            files = release_files(tag, sha, signing)
            release = publish_draft(tag, sha, branch, files)
        verify_public_download(release, tag)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        # CalledProcessError's default text can include Keychain profile references.
        raise SystemExit(f"{Path(error.cmd[0]).name} exited with status {error.returncode}.")
    except (OSError, RuntimeError, ValueError) as error:
        raise SystemExit(str(error))
