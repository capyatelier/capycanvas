import argparse
import copy
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.request

from android_apk import PACKAGE, version_code


class Play:
    def __init__(self, token):
        self.token = token

    def request(self, method, path, body=None):
        media = isinstance(body, bytes)
        prefix = "upload/" if media else ""
        request = urllib.request.Request(
            f"https://androidpublisher.googleapis.com/{prefix}androidpublisher/v3/applications/{PACKAGE}/{path}",
            method=method,
            data=body if media or body is None else json.dumps(body).encode(),
            headers={"Authorization": f"Bearer {self.token}",
                     "Content-Type": "application/octet-stream" if media else "application/json"},
        )
        try:
            with urllib.request.urlopen(request, timeout=180 if media else 60) as response:
                data = response.read()
                return json.loads(data) if data else {}
        except urllib.error.HTTPError as error:
            raise ValueError(f"Google Play {error.code}: {error.read().decode()}") from error


@contextmanager
def editing(play):
    path = "edits/" + play.request("POST", "edits", {})["id"]
    try:
        yield path
        play.request("POST", f"{path}:validate")
        play.request("POST", f"{path}:commit?changesInReviewBehavior=ERROR_IF_IN_REVIEW")
    except Exception:
        try:
            play.request("DELETE", path)
        except Exception as error:
            print(f"Could not discard the failed edit: {error}", file=sys.stderr)
        raise


def refuse_newer(track, code):
    if any(int(item) > int(code) for release in track.get("releases", [])
           for item in release.get("versionCodes", [])):
        raise ValueError("A newer testing build exists; refusing to replace it")


def validate_notes(notes):
    if len(notes) > 500:
        raise ValueError("Google Play release notes cannot exceed 500 Unicode characters")


def upload(play, version, bundle, notes):
    validate_notes(notes)
    code = str(version_code(version))
    data = bundle.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    with editing(play) as path:
        track = play.request("GET", f"{path}/tracks/internal")
        refuse_newer(track, code)
        existing = play.request("GET", f"{path}/bundles").get("bundles", [])
        matches = [item for item in existing if str(item["versionCode"]) == code]
        uploaded = matches[0] if matches else play.request("POST", f"{path}/bundles?uploadType=media", data)
        if str(uploaded["versionCode"]) != code or uploaded.get("sha256") != digest:
            raise ValueError("Play bundle version or SHA-256 differs from the preserved AAB")
        if any(release.get("versionCodes") == [code] and release.get("status") == "completed"
               for release in track.get("releases", [])):
            return
        releases = [release for release in track.get("releases", []) if release.get("status") == "completed"]
        releases.append({"name": version, "versionCodes": [code], "status": "draft",
                         "releaseNotes": [{"language": "en-US", "text": notes}]})
        play.request("PUT", f"{path}/tracks/internal", {"track": "internal", "releases": releases})
    print(f"Preserved and uploaded {version}; APK verification is required before rollout.")


def promote(play, version, track):
    if track not in ("internal", "alpha"):
        raise ValueError("Choose internal testing or the alpha closed-testing track")
    code = str(version_code(version))
    with editing(play) as path:
        source = play.request("GET", f"{path}/tracks/internal")
        target = source if track == "internal" else play.request("GET", f"{path}/tracks/{track}")
        for current in (source, target):
            refuse_newer(current, code)
        releases = [release for release in source.get("releases", [])
                    if release.get("versionCodes") == [code]]
        if len(releases) != 1:
            raise ValueError(f"Version {version} must exist as a single-bundle internal release")
        if track == "alpha" and releases[0].get("status") != "completed":
            raise ValueError("Roll out and test the internal release before closed-beta promotion")
        release = copy.deepcopy(releases[0])
        release.update(name=version, status="completed")
        release.pop("userFraction", None)
        play.request("PUT", f"{path}/tracks/{track}", {"track": track, "releases": [release]})
    print(f"Committed {version} to {track}; Google Play review and availability are separate checks.")


def main():
    parser = argparse.ArgumentParser(description="Upload and promote Android testing releases.")
    commands = parser.add_subparsers(dest="operation", required=True)
    upload_parser = commands.add_parser("upload")
    upload_parser.add_argument("version")
    upload_parser.add_argument("bundle", type=Path)
    promote_parser = commands.add_parser("promote")
    promote_parser.add_argument("version")
    promote_parser.add_argument("track", choices=("internal", "alpha"))
    args = parser.parse_args()
    play = Play(os.environ["TOKEN"])
    if args.operation == "upload":
        upload(play, args.version, args.bundle, os.environ["NOTES"])
    else:
        promote(play, args.version, args.track)


if __name__ == "__main__":
    main()
