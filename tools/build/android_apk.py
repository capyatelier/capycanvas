import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
import urllib.parse
import urllib.request
import zipfile


PACKAGE = "art.capycanvas.editor"
SIGNING_CERTIFICATE = "f48d1c0671a19f2637f43c9140ca61a0c33e82a1792bd6b4e77a774f33521b73"
PLAY_MARKERS = ("com.pairip", "com.android.vending.CHECK_LICENSE", "com.android.vending.licensing")
GOOGLE_TYPES = (b"Lcom/google/android/gms/", b"Lcom/google/firebase/")


def version_code(version):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError(f"Invalid Android version: {version}")
    major, minor, patch = map(int, version.split("."))
    return major * 1_000_000 + minor * 1_000 + patch


def candidates(metadata):
    result = []
    for group in metadata.get("generatedApks", []):
        unprotected = group.get("unprotectedGeneratedStandaloneApks", [])
        if unprotected or group.get("unprotectedGeneratedSplitApks"):
            result.extend(apk["downloadId"] for apk in unprotected)
        elif group.get("generatedUniversalApk"):
            result.append(group["generatedUniversalApk"]["downloadId"])
    return list(dict.fromkeys(result))


def verify_apk(apk, version, build_tools, package=PACKAGE, certificate=SIGNING_CERTIFICATE):
    def run(tool, *args):
        return subprocess.check_output([str(build_tools / tool), *map(str, args)], text=True)

    badging = run("aapt2", "dump", "badging", apk)
    manifest = run("aapt2", "dump", "xmltree", apk, "--file", "AndroidManifest.xml")
    for marker in PLAY_MARKERS:
        if marker in manifest:
            raise ValueError(f"APK requires Google Play: {marker}")
    identity = f"package: name='{package}' versionCode='{version_code(version)}' versionName='{version}'"
    if not badging.startswith(identity + " ") and not badging.startswith(identity + "\n"):
        raise ValueError("APK package or version does not match the release")
    if "minSdkVersion:'29'\n" not in badging or "maxSdkVersion:" in badging:
        raise ValueError("APK does not cover the supported Android versions (API 29 and later)")
    if (re.search(r"\bsplit=", badging.splitlines()[0]) or "uses-split" in manifest
            or re.search(r"isSplitRequired[^\n]*=true", manifest)):
        raise ValueError("APK requires additional split APKs")
    if "native-code: 'arm64-v8a'\n" not in badging:
        raise ValueError("APK does not contain the release arm64-v8a libraries")
    with zipfile.ZipFile(apk) as archive:
        if "lib/arm64-v8a/liblayer_android.so" not in archive.namelist():
            raise ValueError("APK is missing the Android renderer")
        if "classes.dex" not in archive.namelist():
            raise ValueError("APK is missing the Android application")
        for name in archive.namelist():
            if name.endswith(".dex"):
                dex = archive.read(name)
                for marker in PLAY_MARKERS:
                    if marker.encode() in dex or marker.replace(".", "/").encode() in dex:
                        raise ValueError(f"APK contains Google Play code: {marker}")
                for descriptor in GOOGLE_TYPES:
                    if descriptor in dex:
                        raise ValueError(f"APK contains Google service classes: {descriptor.decode()}")
    signatures = run("apksigner", "verify", "--min-sdk-version", "29", "--max-sdk-version", "29",
                     "--print-certs", apk)
    signers = re.findall(r"^(?!Source Stamp)[^\n]*certificate SHA-256 digest: ([0-9a-f]{64})$",
                         signatures, re.MULTILINE)
    if signers != [certificate.lower()]:
        raise ValueError("APK signing certificate would break updates from existing installations")
    run("apksigner", "verify", apk)


def fetch(url, token):
    request = urllib.request.Request(url, headers={"Authorization": f"Bearer {token}"})
    return urllib.request.urlopen(request, timeout=60)


def download_apk(version, output, build_tools, token):
    api = f"https://androidpublisher.googleapis.com/androidpublisher/v3/applications/{PACKAGE}/generatedApks/{version_code(version)}"
    output.parent.mkdir(parents=True, exist_ok=True)
    rejected = {}
    with tempfile.TemporaryDirectory(prefix=".android-apk-", dir=output.parent) as directory:
        candidate = Path(directory) / "candidate.apk"
        for attempt in range(30):
            with fetch(api, token) as response:
                metadata = json.load(response)
            for download_id in candidates(metadata):
                if download_id in rejected:
                    continue
                url = f"{api}/downloads/{urllib.parse.quote(download_id, safe='')}:download?alt=media"
                with fetch(url, token) as response, candidate.open("wb") as target:
                    shutil.copyfileobj(response, target)
                try:
                    verify_apk(candidate, version, build_tools)
                except (ValueError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
                    rejected[download_id] = str(error)
                    print(f"Rejected generated APK: {error}", flush=True)
                    continue
                candidate.replace(output)
                print(f"Verified APK without Google Play checks: {output}")
                return
            if attempt < 29:
                print(f"Waiting for an APK without Google Play checks ({attempt + 1}/30)", flush=True)
                time.sleep(20)
    reasons = "; ".join(dict.fromkeys(rejected.values())) or "no unprotected standalone APK was generated"
    raise ValueError(f"No compatible APK: {reasons}. Turn off Automatic protection for the release in Play Console before uploading its bundle.")


def main():
    parser = argparse.ArgumentParser(description="Download or verify a Play-signed APK that runs without Google Play.")
    parser.add_argument("operation", choices=("download", "verify"))
    parser.add_argument("version")
    parser.add_argument("apk", type=Path)
    parser.add_argument("--build-tools", type=Path,
                        default=Path(os.environ.get("ANDROID_HOME", Path.home() / "Android/Sdk")) / "build-tools/37.0.0")
    args = parser.parse_args()
    if args.operation == "download":
        download_apk(args.version, args.apk, args.build_tools, os.environ["TOKEN"])
    else:
        verify_apk(args.apk, args.version, args.build_tools)


if __name__ == "__main__":
    main()
