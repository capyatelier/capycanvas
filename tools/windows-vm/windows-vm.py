#!/usr/bin/env python3
"""Manage a Windows 11 evaluation VM that builds the Windows client and runs GPU-free tests.

See docs/development/windows-vm.md.
"""

import argparse
import getpass
import json
import os
from pathlib import Path
import secrets
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import time
from urllib.parse import unquote, urlparse


ROOT = Path(__file__).resolve().parents[2]
GUEST_FILES = Path(__file__).resolve().parent
PROGRAM = sys.argv[0]
ISO_URL = "https://aka.ms/Win11E-ISO-25H2-en-us"
STATE = Path(os.environ.get("CAPYCANVAS_VM_DIR") or Path(
    os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share") / "capycanvas/windows-vm")
CPUS = os.environ.get("CAPYCANVAS_VM_CPUS") or str(min(8, os.cpu_count() or 1))
MEMORY = os.environ.get("CAPYCANVAS_VM_MEMORY") or "16G"
DISK_SIZE = "128G"
USER = "capy"
SEED_LABEL = "CAPYSEED"
GUEST_REPO = "C:\\capycanvas"
ISO_RECORD = STATE / "iso"
FIRMWARE = STATE / "firmware.json"
KEY = STATE / "id_ed25519"
KNOWN_HOSTS = STATE / "known_hosts"
PASSWORD = STATE / "password"
INSTALL = STATE / "install"
BASE = STATE / "base"
RUN = STATE / "run"
QMP_SOCKET = STATE / "qmp.sock"
TPM_SOCKET = STATE / "swtpm.sock"
QEMU_PID = STATE / "qemu.pid"
TPM_PID = STATE / "swtpm.pid"
SSH_PORT = STATE / "ssh-port"
COMMANDS = ("qemu-system-x86_64", "qemu-img", "swtpm", "xorriso", "ssh", "ssh-keygen", "curl")
PACKAGES = {
    "fedora": [["dnf", "install", "-y", "qemu-system-x86", "qemu-img", "qemu-ui-gtk", "edk2-ovmf",
                "swtpm", "xorriso", "openssh-clients", "curl"]],
    "debian": [["apt-get", "update"],
               ["apt-get", "install", "-y", "qemu-system-x86", "qemu-utils", "qemu-system-gui", "ovmf",
                "swtpm", "xorriso", "openssh-client", "curl"]],
    "arch": [["pacman", "-S", "--needed", "--noconfirm", "qemu-system-x86", "qemu-img", "qemu-ui-gtk",
              "edk2-ovmf", "swtpm", "libisoburn", "openssh", "curl"]],
}
HYPERV = ("hv-relaxed,hv-vapic,hv-spinlocks=0x1fff,hv-vpindex,hv-runtime,hv-synic,hv-stimer,"
          "hv-time,hv-frequencies,hv-tlbflush,hv-ipi")
SYNC = (f"$repo = '{GUEST_REPO}'; New-Item -ItemType Directory -Force $repo | Out-Null; "
        "Get-ChildItem -Force $repo | Where-Object Name -NotIn 'target', 'artifacts' | "
        "Remove-Item -Recurse -Force; tar -xf - -C $repo; exit $LASTEXITCODE")


def run(*command, check=True, **options):
    return subprocess.run([str(part) for part in command], check=check, **options)


def remove(path):
    if path.is_dir():
        shutil.rmtree(path)
    else:
        path.unlink(missing_ok=True)


def firmware():
    descriptors = {}
    for directory in ("/usr/share/qemu/firmware", "/etc/qemu/firmware"):
        descriptors.update((path.name, path) for path in Path(directory).glob("*.json"))
    candidates = []
    for name in sorted(descriptors):
        text = descriptors[name].read_text()
        description = json.loads(text) if text.strip() else {}
        mapping = description.get("mapping", {})
        features = description.get("features", [])
        if ("uefi" in description.get("interface-types", []) and mapping.get("device") == "flash"
                and "nvram-template" in mapping and "secure-boot" in features
                and any(target.get("architecture") == "x86_64"
                        and any(machine.startswith("pc-q35") for machine in target.get("machines", []))
                        for target in description.get("targets", []))):
            candidates.append(("enrolled-keys" not in features, mapping))
    if not candidates:
        return None
    mapping = min(candidates, key=lambda candidate: candidate[0])[1]
    return {"code": mapping["executable"]["filename"],
            "code_format": mapping["executable"].get("format", "raw"),
            "vars": mapping["nvram-template"]["filename"],
            "vars_format": mapping["nvram-template"].get("format", "raw")}


def ready():
    if not all(map(shutil.which, COMMANDS)):
        return False
    displays = run("qemu-system-x86_64", "-display", "help", capture_output=True, text=True).stdout
    return "gtk" in displays.split() and firmware() is not None


def install_packages():
    if ready():
        return
    release = dict(line.split("=", 1) for line in Path("/etc/os-release").read_text().splitlines()
                   if "=" in line)
    distributions = f"{release.get('ID', '')} {release.get('ID_LIKE', '')}".replace('"', "").split()
    family = next((name for name in PACKAGES if name in distributions), None)
    if family is None:
        sys.exit("Install QEMU with its GTK display, OVMF, swtpm, xorriso, OpenSSH and curl, "
                 "then rerun setup.")
    sudo = [] if os.geteuid() == 0 else ["sudo"]
    for command in PACKAGES[family]:
        run(*sudo, *command)
    if not ready():
        sys.exit("QEMU, swtpm, xorriso, OpenSSH, curl or Secure Boot OVMF firmware is still missing.")


def ensure_kvm_access():
    kvm = Path("/dev/kvm")
    if not kvm.exists():
        sys.exit("/dev/kvm is missing; enable hardware virtualization and load the KVM module.")
    if not os.access(kvm, os.R_OK | os.W_OK):
        user = getpass.getuser()
        run("sudo", "usermod", "-aG", kvm.group(), user)
        sys.exit(f"Added {user} to the {kvm.group()} group. Log in again, then rerun setup.")


def downloads():
    if shutil.which("xdg-user-dir"):
        path = run("xdg-user-dir", "DOWNLOAD", capture_output=True, text=True).stdout.strip()
        if path and Path(path) != Path.home():
            return Path(path)
    return Path.home() / "Downloads"


def download_iso():
    url = run("curl", "-fsSIL", "-o", "/dev/null", "-w", "%{url_effective}", ISO_URL,
              capture_output=True, text=True).stdout
    name = unquote(Path(urlparse(url).path).name)
    if not name.endswith(".iso"):
        sys.exit(f"{ISO_URL} did not redirect to an ISO: {url}")
    iso = downloads() / name
    if not iso.exists():
        iso.parent.mkdir(parents=True, exist_ok=True)
        partial = iso.with_name(f"{name}.part")
        run("curl", "-fL", "-C", "-", "-o", partial, url)
        partial.rename(iso)
    return iso


def setup(args):
    install_packages()
    ensure_kvm_access()
    iso = args.iso.resolve() if args.iso else download_iso()
    if not iso.is_file():
        sys.exit(f"{iso} is not a file.")
    STATE.mkdir(mode=0o700, parents=True, exist_ok=True)
    if shutil.which("chattr"):
        subprocess.run(["chattr", "+C", STATE], stderr=subprocess.DEVNULL)
    ISO_RECORD.write_text(f"{iso}\n")
    print(f"Windows ISO: {iso}\nNext: {PROGRAM} create")


def process(pidfile, name):
    try:
        pid = int(pidfile.read_text())
        return pid if Path(f"/proc/{pid}/comm").read_text().strip() == name else None
    except (FileNotFoundError, ValueError):
        return None


def running():
    return process(QEMU_PID, "qemu-system-x86") is not None


def require_running():
    if not running():
        sys.exit(f"The VM is not running; run `{PROGRAM} start`.")


def wait_for_exit(pid, timeout):
    deadline = time.monotonic() + timeout
    while Path(f"/proc/{pid}").exists():
        if time.monotonic() > deadline:
            return False
        time.sleep(1)
    return True


def terminate(pidfile, name):
    pid = process(pidfile, name)
    if pid:
        os.kill(pid, signal.SIGTERM)
        wait_for_exit(pid, 10)


def qmp(command, **arguments):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.connect(str(QMP_SOCKET))
        stream = connection.makefile("rw")
        stream.readline()
        for message in ({"execute": "qmp_capabilities"}, {"execute": command, "arguments": arguments}):
            stream.write(json.dumps(message) + "\n")
            stream.flush()
            reply = {}
            while not reply.keys() & {"return", "error"}:
                line = stream.readline()
                if not line:
                    return
                reply = json.loads(line)
            if "error" in reply:
                sys.exit(f"QEMU {command} failed: {reply['error']['desc']}")


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def launch(machine, cdroms=(), gui=False):
    terminate(TPM_PID, "swtpm")
    selected = json.loads(FIRMWARE.read_text())
    port = free_port()
    SSH_PORT.write_text(f"{port}\n")
    run("swtpm", "socket", "--tpm2", "--tpmstate", f"dir={machine / 'tpm'}",
        "--ctrl", f"type=unixio,path={TPM_SOCKET}", "--pid", f"file={TPM_PID}", "--terminate", "--daemon")
    command = [
        "qemu-system-x86_64", "-name", "capycanvas-windows", "-nodefaults",
        "-machine", "q35,accel=kvm,smm=on", "-global", "driver=cfi.pflash01,property=secure,value=on",
        "-cpu", f"host,{HYPERV}", "-smp", CPUS, "-m", MEMORY, "-rtc", "base=utc",
        "-drive", f"if=pflash,unit=0,readonly=on,format={selected['code_format']},file={selected['code']}",
        "-drive", f"if=pflash,unit=1,format={selected['vars_format']},file={machine / 'efivars'}",
        "-chardev", f"socket,id=tpm,path={TPM_SOCKET}", "-tpmdev", "emulator,id=tpm,chardev=tpm",
        "-device", "tpm-crb,tpmdev=tpm",
        "-drive", f"if=none,id=disk,format=qcow2,discard=unmap,file={machine / 'disk.qcow2'}",
        "-device", "nvme,drive=disk,serial=capycanvas,bootindex=0",
        "-netdev", f"user,id=net,hostfwd=tcp:127.0.0.1:{port}-:22", "-device", "e1000e,netdev=net",
        "-device", "qemu-xhci", "-device", "usb-tablet", "-device", "VGA",
        "-qmp", f"unix:{QMP_SOCKET},server=on,wait=off", "-display", "gtk" if gui else "none",
        "-daemonize", "-pidfile", QEMU_PID,
    ]
    for index, image in enumerate(cdroms):
        command += ["-drive", f"if=none,id=cd{index},media=cdrom,readonly=on,file={image}",
                    "-device", f"ide-cd,drive=cd{index},bus=ide.{index},bootindex={index + 1}"]
    try:
        run(*command)
    except subprocess.CalledProcessError:
        terminate(TPM_PID, "swtpm")
        raise


def ssh_command(*remote):
    return ["ssh", "-i", str(KEY), "-p", SSH_PORT.read_text().strip(), "-o", "IdentitiesOnly=yes",
            "-o", "BatchMode=yes", "-o", f"UserKnownHostsFile={KNOWN_HOSTS}",
            "-o", "StrictHostKeyChecking=accept-new", "-o", "HostKeyAlias=capycanvas-windows-vm",
            "-o", "ConnectTimeout=10", "-o", "LogLevel=ERROR", f"{USER}@127.0.0.1", *remote]


def guest(command):
    run(*ssh_command(command))


def wait_for_ssh(timeout):
    deadline = time.monotonic() + timeout
    while run(*ssh_command("exit"), stdin=subprocess.DEVNULL, capture_output=True,
              check=False).returncode:
        if not running():
            sys.exit("The VM stopped before SSH became available.")
        if time.monotonic() > deadline:
            sys.exit(f"SSH did not become available; inspect the VM with `{PROGRAM} screenshot`.")
        time.sleep(5)


def create(args):
    if (BASE / "disk.qcow2").exists():
        sys.exit(f"The VM already exists; run `{PROGRAM} destroy` to rebuild it.")
    if running():
        sys.exit("A VM is running.")
    iso = Path(ISO_RECORD.read_text().strip()) if ISO_RECORD.exists() else None
    if iso is None or not iso.is_file():
        sys.exit(f"The Windows ISO is missing; run `{PROGRAM} setup`.")
    selected = firmware()
    if selected is None:
        sys.exit(f"No Secure Boot OVMF firmware was found; run `{PROGRAM} setup`.")
    FIRMWARE.write_text(json.dumps(selected))
    remove(INSTALL)
    seed = INSTALL / "seed"
    (INSTALL / "tpm").mkdir(parents=True)
    seed.mkdir()
    shutil.copyfile(selected["vars"], INSTALL / "efivars")
    run("qemu-img", "create", "-q", "-f", "qcow2", INSTALL / "disk.qcow2", DISK_SIZE)
    if not KEY.exists():
        run("ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "capycanvas-windows-vm", "-f", KEY)
    password = secrets.token_urlsafe(18)
    PASSWORD.write_text(f"{password}\n")
    unattend = (GUEST_FILES / "autounattend.xml").read_text().replace("@PASSWORD@", password)
    (seed / "autounattend.xml").write_text(unattend)
    for name in ("bootstrap.ps1", "provision.ps1"):
        shutil.copy(GUEST_FILES / name, seed)
    shutil.copy(KEY.with_suffix(".pub"), seed / "administrators_authorized_keys")
    image = run("xorriso", "-as", "mkisofs", "-quiet", "-J", "-joliet-long", "-r", "-V", SEED_LABEL,
                "-o", INSTALL / "seed.iso", seed, capture_output=True, text=True, check=False)
    if image.returncode:
        sys.exit(image.stderr)
    launch(INSTALL, [iso, INSTALL / "seed.iso"], args.gui)
    print("Installing Windows and the build tools; this takes a while.", flush=True)
    for _ in range(30):
        qmp("send-key", keys=[{"type": "qcode", "data": "ret"}])
        time.sleep(1)
    wait_for_ssh(3 * 60 * 60)
    guest(f"& ('{{0}}:\\provision.ps1' -f (Get-Volume -FileSystemLabel {SEED_LABEL}).DriveLetter)")
    stop(args)
    remove(seed)
    remove(INSTALL / "seed.iso")
    INSTALL.rename(BASE)
    (BASE / "disk.qcow2").chmod(0o444)
    print(f"Created the base image. Next: {PROGRAM} check")


def boot(gui):
    if running():
        return
    if not (BASE / "disk.qcow2").exists():
        sys.exit(f"The VM does not exist; run `{PROGRAM} create`.")
    if not RUN.exists():
        fresh = STATE / "run.partial"
        remove(fresh)
        shutil.copytree(BASE, fresh, ignore=shutil.ignore_patterns("disk.qcow2"))
        run("qemu-img", "create", "-q", "-f", "qcow2", "-F", "qcow2", "-b", BASE / "disk.qcow2",
            fresh / "disk.qcow2")
        fresh.rename(RUN)
    launch(RUN, gui=gui)
    wait_for_ssh(10 * 60)


def start(args):
    boot(args.gui)
    print(f"The VM is running. Connect with `{PROGRAM} ssh`.")


def stop(args):
    pid = process(QEMU_PID, "qemu-system-x86")
    if pid is None:
        return
    qmp("system_powerdown")
    if not wait_for_exit(pid, 5 * 60):
        qmp("quit")
        wait_for_exit(pid, 30)


def ssh(args):
    require_running()
    os.execvp("ssh", ssh_command(*args.command))


def sync(args):
    require_running()
    listing = run("git", "ls-files", "-z", "--cached", "--others", "--exclude-standard",
                  cwd=ROOT, capture_output=True).stdout
    files = b"".join(name + b"\0" for name in listing.split(b"\0")
                     if name and os.path.lexists(ROOT / os.fsdecode(name)))
    archive = subprocess.Popen(["tar", "--null", "--files-from=-", "--create", "--file=-"], cwd=ROOT,
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    extract = subprocess.Popen(ssh_command(SYNC), stdin=archive.stdout)
    archive.stdout.close()
    archive.stdin.write(files)
    archive.stdin.close()
    if archive.wait() or extract.wait():
        sys.exit("Copying the working tree to the VM failed.")


def check(args):
    boot(gui=False)
    sync(args)
    configuration = "Release" if args.release else "Debug"
    guest(f"& '{GUEST_REPO}\\apps\\layer-windows\\scripts\\test-without-gpu.ps1' "
          f"-Configuration {configuration}")


def screenshot(args):
    require_running()
    qmp("screendump", filename=str(args.file.resolve()), format="png")


def reset(args):
    if running():
        sys.exit(f"Stop the VM first with `{PROGRAM} stop`.")
    remove(RUN)


def destroy(args):
    stop(args)
    for path in (INSTALL, BASE, RUN, STATE / "run.partial", FIRMWARE, PASSWORD, KNOWN_HOSTS):
        remove(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(required=True, metavar="command")

    def command(name, handler, summary):
        subcommand = commands.add_parser(name, help=summary, description=summary)
        subcommand.set_defaults(handler=handler)
        return subcommand

    command("setup", setup, "install host packages, grant KVM access and download the ISO").add_argument(
        "--iso", type=Path, help="use this ISO instead of downloading one")
    command("create", create, "install Windows and the build tools into the base image").add_argument(
        "--gui", action="store_true", help="show the VM display in a window")
    command("start", start, "boot the VM").add_argument(
        "--gui", action="store_true", help="show the VM display in a window")
    command("stop", stop, "shut the VM down")
    command("ssh", ssh, "open a PowerShell session or run a command in the VM").add_argument(
        "command", nargs=argparse.REMAINDER)
    command("sync", sync, f"copy the working tree to {GUEST_REPO}, keeping build outputs")
    command("check", check, "sync, build and run the tests that need no GPU").add_argument(
        "--release", action="store_true", help="build the Release configuration")
    command("screenshot", screenshot, "save the VM display as a PNG").add_argument("file", type=Path)
    command("reset", reset, "discard every change made since create")
    command("destroy", destroy, "delete the VM, keeping the downloaded ISO")
    args = parser.parse_args()
    try:
        args.handler(args)
    except subprocess.CalledProcessError as error:
        sys.exit(f"Exit status {error.returncode}: {shlex.join(map(str, error.cmd))}")


if __name__ == "__main__":
    main()
