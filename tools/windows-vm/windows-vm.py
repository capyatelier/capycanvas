#!/usr/bin/env python3
"""Manage Windows 11 evaluation VMs that build the Windows client and run its tests.

See docs/development/windows-vm.md.
"""

import argparse
import base64
import fcntl
import getpass
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from urllib.parse import unquote, urlparse


ROOT = Path(__file__).resolve().parents[2]
GUEST_FILES = Path(__file__).resolve().parent
PROGRAM = sys.argv[0]
ISO_URL = "https://aka.ms/Win11E-ISO-25H2-en-us"
STATE = Path(os.environ.get("CAPYCANVAS_VM_DIR") or Path(
    os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share") / "capycanvas/windows-vm")
RUNTIME = Path(os.environ.get("XDG_RUNTIME_DIR") or tempfile.gettempdir()) / (
    f"capycanvas-vm-{os.getuid()}-{hashlib.sha256(bytes(STATE)).hexdigest()[:8]}")
CPUS = os.environ.get("CAPYCANVAS_VM_CPUS") or str(min(8, os.cpu_count() or 1))
MEMORY = os.environ.get("CAPYCANVAS_VM_MEMORY") or "16G"
DISPLAY = os.environ.get("CAPYCANVAS_VM_DISPLAY") or "2560x1600"
VGA = "VGA,xres={},yres={},vgamem_mb=64".format(*DISPLAY.split("x"))
DISK_SIZE = "128G"
USER = "capy"
SEED_LABEL = "CAPYSEED"
GUEST_REPO = "C:\\capycanvas"
GUEST_PWSH = "C:\\Program Files\\PowerShell\\7\\pwsh.exe"
DESKTOP_TASK = "capycanvas-desktop"
SOFTWARE_BUILD = f"{GUEST_REPO}\\artifacts\\windows\\SoftwareAdapter"
ISO_RECORD = STATE / "iso"
FIRMWARE = STATE / "firmware.json"
KEY = STATE / "id_ed25519"
KNOWN_HOSTS = STATE / "known_hosts"
PASSWORD = STATE / "password"
INSTALL = STATE / "install"
BASE = STATE / "base"
VMS = STATE / "vms"
LEGACY_RUN = STATE / "run"
LEGACY_PID = STATE / "qemu.pid"
VM_NAME = re.compile(r"[a-z0-9][a-z0-9-]{0,31}")
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
CLEAR = (f"New-Item -ItemType Directory -Force {GUEST_REPO} | Out-Null; Get-ChildItem -Force {GUEST_REPO} | "
         "Where-Object Name -NotIn 'target', 'artifacts' | Remove-Item -Recurse -Force; ")
EXTRACT = f"tar -xmf - -C {GUEST_REPO}; exit $LASTEXITCODE"


class Machine:
    def __init__(self, name, directory):
        self.name = name
        self.directory = directory
        self.runtime = RUNTIME / name
        self.qmp_socket = self.runtime / "qmp.sock"
        self.tpm_socket = self.runtime / "swtpm.sock"
        self.qemu_pid = self.runtime / "qemu.pid"
        self.tpm_pid = self.runtime / "swtpm.pid"
        self.ssh_port = self.runtime / "ssh-port"
        self.prepared = directory / "prepared"


def installer():
    return Machine(".install", INSTALL)


def machines():
    return [Machine(path.name, path) for path in sorted(VMS.glob("*")) if VM_NAME.fullmatch(path.name)]


def selected(args):
    name = args.vm or os.environ.get("CAPYCANVAS_VM") or (
        re.sub(r"[^a-z0-9]+", "-", ROOT.name.lower()).strip("-")[:32] or "default")
    if not VM_NAME.fullmatch(name):
        sys.exit(f"VM names use lowercase letters, digits and hyphens, up to 32 characters: {name}")
    return Machine(name, VMS / name)


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


def running(machine):
    return process(machine.qemu_pid, "qemu-system-x86") is not None


def require_running(machine):
    if not running(machine):
        sys.exit(f"The {machine.name} VM is not running; run `{PROGRAM} --vm {machine.name} start`.")


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


def qmp(machine, command, **arguments):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.connect(str(machine.qmp_socket))
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
    for directory in (RUNTIME, machine.runtime):
        directory.mkdir(mode=0o700, exist_ok=True)
    terminate(machine.tpm_pid, "swtpm")
    selected_firmware = json.loads(FIRMWARE.read_text())
    port = free_port()
    machine.ssh_port.write_text(f"{port}\n")
    run("swtpm", "socket", "--tpm2", "--tpmstate", f"dir={machine.directory / 'tpm'}",
        "--ctrl", f"type=unixio,path={machine.tpm_socket}", "--pid", f"file={machine.tpm_pid}",
        "--terminate", "--daemon")
    command = [
        "qemu-system-x86_64", "-name", f"capycanvas-windows-{machine.name.lstrip('.')}", "-nodefaults",
        "-machine", "q35,accel=kvm,smm=on", "-global", "driver=cfi.pflash01,property=secure,value=on",
        "-cpu", f"host,{HYPERV}", "-smp", CPUS, "-m", MEMORY, "-rtc", "base=utc",
        "-drive", f"if=pflash,unit=0,readonly=on,format={selected_firmware['code_format']},"
                  f"file={selected_firmware['code']}",
        "-drive", f"if=pflash,unit=1,format={selected_firmware['vars_format']},file={machine.directory / 'efivars'}",
        "-chardev", f"socket,id=tpm,path={machine.tpm_socket}", "-tpmdev", "emulator,id=tpm,chardev=tpm",
        "-device", "tpm-crb,tpmdev=tpm",
        "-drive", f"if=none,id=disk,format=qcow2,discard=unmap,file={machine.directory / 'disk.qcow2'}",
        "-device", "nvme,drive=disk,serial=capycanvas,bootindex=0",
        "-netdev", f"user,id=net,hostfwd=tcp:127.0.0.1:{port}-:22", "-device", "e1000e,netdev=net",
        "-device", "qemu-xhci", "-device", "usb-tablet", "-device", VGA,
        "-qmp", f"unix:{machine.qmp_socket},server=on,wait=off", "-display", "gtk" if gui else "none",
        "-daemonize", "-pidfile", machine.qemu_pid,
    ]
    for index, image in enumerate(cdroms):
        command += ["-drive", f"if=none,id=cd{index},media=cdrom,readonly=on,file={image}",
                    "-device", f"ide-cd,drive=cd{index},bus=ide.{index},bootindex={index + 1}"]
    try:
        run(*command)
    except subprocess.CalledProcessError:
        terminate(machine.tpm_pid, "swtpm")
        raise


def ssh_command(machine, *remote):
    return ["ssh", "-i", str(KEY), "-p", machine.ssh_port.read_text().strip(), "-o", "IdentitiesOnly=yes",
            "-o", "BatchMode=yes", "-o", f"UserKnownHostsFile={KNOWN_HOSTS}",
            "-o", "StrictHostKeyChecking=accept-new", "-o", "HostKeyAlias=capycanvas-windows-vm",
            "-o", "ConnectTimeout=30", "-o", "LogLevel=ERROR", f"{USER}@127.0.0.1", *remote]


def guest(machine, command, **options):
    return run(*ssh_command(machine, command), **options)


def guest_script(machine, script):
    encoded = base64.b64encode(script.encode("utf-16-le")).decode()
    status = guest(machine, f"powershell -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand {encoded}",
                   check=False).returncode
    if status:
        sys.exit(f"A PowerShell script in the {machine.name} VM exited with status {status}.")


def wait_for_ssh(machine, timeout):
    deadline = time.monotonic() + timeout
    while run(*ssh_command(machine, "exit"), stdin=subprocess.DEVNULL, capture_output=True,
              check=False).returncode:
        if not running(machine):
            sys.exit("The VM stopped before SSH became available.")
        if time.monotonic() > deadline:
            sys.exit(f"SSH did not become available; inspect the VM with `{PROGRAM} screenshot`.")
        time.sleep(5)


def create(args):
    if (BASE / "disk.qcow2").exists():
        sys.exit(f"The base image already exists; run `{PROGRAM} destroy` to rebuild it.")
    machine = installer()
    if running(machine):
        sys.exit("A base image install is running.")
    iso = Path(ISO_RECORD.read_text().strip()) if ISO_RECORD.exists() else None
    if iso is None or not iso.is_file():
        sys.exit(f"The Windows ISO is missing; run `{PROGRAM} setup`.")
    selected_firmware = firmware()
    if selected_firmware is None:
        sys.exit(f"No Secure Boot OVMF firmware was found; run `{PROGRAM} setup`.")
    FIRMWARE.write_text(json.dumps(selected_firmware))
    remove(INSTALL)
    seed = INSTALL / "seed"
    (INSTALL / "tpm").mkdir(parents=True)
    seed.mkdir()
    shutil.copyfile(selected_firmware["vars"], INSTALL / "efivars")
    run("qemu-img", "create", "-q", "-f", "qcow2", INSTALL / "disk.qcow2", DISK_SIZE)
    if not KEY.exists():
        run("ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "capycanvas-windows-vm", "-f", KEY)
    password = secrets.token_urlsafe(18)
    PASSWORD.write_text(f"{password}\n")
    unattend = (GUEST_FILES / "autounattend.xml").read_text().replace("@PASSWORD@", password)
    (seed / "autounattend.xml").write_text(unattend)
    for name in ("bootstrap.ps1", "provision.ps1", "prepare.ps1"):
        shutil.copy(GUEST_FILES / name, seed)
    shutil.copy(KEY.with_suffix(".pub"), seed / "administrators_authorized_keys")
    image = run("xorriso", "-as", "mkisofs", "-quiet", "-J", "-joliet-long", "-r", "-V", SEED_LABEL,
                "-o", INSTALL / "seed.iso", seed, capture_output=True, text=True, check=False)
    if image.returncode:
        sys.exit(image.stderr)
    launch(machine, [iso, INSTALL / "seed.iso"], args.gui)
    print("Installing Windows and the build tools; this takes a while.", flush=True)
    for _ in range(30):
        qmp(machine, "send-key", keys=[{"type": "qcode", "data": "ret"}])
        time.sleep(1)
    wait_for_ssh(machine, 3 * 60 * 60)
    guest(machine, f"& ('{{0}}:\\provision.ps1' -f (Get-Volume -FileSystemLabel {SEED_LABEL}).DriveLetter)")
    stop_machine(machine)
    remove(seed)
    remove(INSTALL / "seed.iso")
    INSTALL.rename(BASE)
    (BASE / "disk.qcow2").chmod(0o444)
    print(f"Created the base image. Next: {PROGRAM} check")


def restart(machine):
    def booted():
        return guest(machine, "(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.Ticks", stdin=subprocess.DEVNULL,
                     capture_output=True, text=True, check=False).stdout.strip()
    previous = booted()
    guest(machine, "Restart-Computer -Force", check=False)
    deadline = time.monotonic() + 10 * 60
    while booted() in ("", previous):
        if time.monotonic() > deadline:
            sys.exit(f"The {machine.name} VM did not restart.")
        time.sleep(3)


def prepare(machine):
    script = (GUEST_FILES / "prepare.ps1").read_text()
    digest = hashlib.sha256(f"{VGA}\n{script}".encode()).hexdigest()
    if machine.prepared.exists() and machine.prepared.read_text().strip() == digest:
        return
    guest_script(machine, script)
    restart(machine)
    machine.prepared.write_text(f"{digest}\n")


def boot(machine, gui):
    if running(machine):
        return
    if not (BASE / "disk.qcow2").exists():
        sys.exit(f"The base image does not exist; run `{PROGRAM} create`.")
    if not machine.directory.exists():
        fresh = machine.directory.with_name(f"{machine.name}.partial")
        remove(fresh)
        shutil.copytree(BASE, fresh, ignore=shutil.ignore_patterns("disk.qcow2"))
        run("qemu-img", "create", "-q", "-f", "qcow2", "-F", "qcow2", "-b", BASE / "disk.qcow2",
            fresh / "disk.qcow2")
        fresh.rename(machine.directory)
    launch(machine, gui=gui)
    wait_for_ssh(machine, 10 * 60)
    prepare(machine)


def start(args):
    machine = selected(args)
    boot(machine, args.gui)
    print(f"The {machine.name} VM is running. Connect with `{PROGRAM} --vm {machine.name} ssh`.")


def stop_machine(machine):
    pid = process(machine.qemu_pid, "qemu-system-x86")
    if pid is None:
        return
    qmp(machine, "system_powerdown")
    if not wait_for_exit(pid, 5 * 60):
        qmp(machine, "quit")
        wait_for_exit(pid, 30)


def stop(args):
    stop_machine(selected(args))


def ssh(args):
    machine = selected(args)
    require_running(machine)
    os.execvp("ssh", ssh_command(machine, *args.command))


def claim(machine):
    lock = open(machine.directory / "lock", "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        sys.exit(f"Another command is using the {machine.name} VM; pass --vm to use another one.")
    return lock


def sync_machine(machine):
    require_running(machine)
    listing = run("git", "ls-files", "-z", "--cached", "--others", "--exclude-standard",
                  cwd=ROOT, capture_output=True).stdout
    hashes = {}
    for name in listing.decode().split("\0"):
        path = ROOT / name
        if name and os.path.lexists(path):
            content = os.readlink(path).encode() if path.is_symlink() else path.read_bytes()
            hashes[name] = hashlib.sha256(content).hexdigest()
    record = machine.directory / "synced.json"
    synced = json.loads(record.read_text()) if record.exists() else {}
    full = not synced or not synced.keys() <= hashes.keys()
    files = "".join(f"{name}\0" for name, digest in hashes.items() if full or synced.get(name) != digest)
    record.unlink(missing_ok=True)
    archive = subprocess.Popen(["tar", "--null", "--files-from=-", "--create", "--file=-"], cwd=ROOT,
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    extract = subprocess.Popen(ssh_command(machine, (CLEAR if full else "") + EXTRACT), stdin=archive.stdout)
    archive.stdout.close()
    archive.stdin.write(files.encode())
    archive.stdin.close()
    if archive.wait() or extract.wait():
        sys.exit("Copying the working tree to the VM failed.")
    record.write_text(json.dumps(hashes))


def sync(args):
    machine = selected(args)
    with claim(machine):
        sync_machine(machine)


def check(args):
    machine = selected(args)
    boot(machine, gui=False)
    lock = claim(machine)
    sync_machine(machine)
    configuration = "Release" if args.release else "Debug"
    guest(machine, f"& '{GUEST_REPO}\\apps\\layer-windows\\scripts\\test-without-gpu.ps1' "
                   f"-Configuration {configuration}")


def desktop(machine, arguments):
    guest_script(machine, f"""$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
while (!(Get-Process explorer -IncludeUserName -ErrorAction SilentlyContinue | Where-Object UserName -Like '*\\{USER}')) {{ Start-Sleep 2 }}
$action = New-ScheduledTaskAction -Execute conhost.exe -Argument '"{GUEST_PWSH}" -NoProfile -Sta -ExecutionPolicy Bypass {arguments}' -WorkingDirectory '{GUEST_REPO}'
$principal = New-ScheduledTaskPrincipal -UserId {USER} -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit 0
Register-ScheduledTask {DESKTOP_TASK} -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask {DESKTOP_TASK}
""")


def desktop_running(machine):
    state = guest(machine, f"(Get-ScheduledTask {DESKTOP_TASK}).State", capture_output=True, text=True, check=False)
    return state.returncode != 0 or state.stdout.strip() == "Running"


def fixtures(args):
    machine = selected(args)
    boot(machine, gui=False)
    lock = claim(machine)
    sync_machine(machine)
    if not args.no_build:
        guest(machine, f"& '{GUEST_REPO}\\apps\\layer-windows\\scripts\\build.ps1' -Configuration Release "
                       f"-SoftwareAdapterTests -OutputDirectory '{SOFTWARE_BUILD}'")
    name = time.strftime("%Y%m%d-%H%M%S")
    output = f"{GUEST_REPO}\\artifacts\\windows\\vm-fixtures\\{name}"
    selection = f" -Name {','.join(args.names)}" if args.names else ""
    desktop(machine, f"-File {GUEST_REPO}\\tools\\windows-vm\\run-fixtures.ps1 "
                     f"-Executable {SOFTWARE_BUILD}\\CapyCanvas.exe -Output {output}{selection}")
    results = []
    finished = False
    while not finished:
        time.sleep(15)
        finished = not desktop_running(machine)
        lines = guest(machine, f"Get-Content -ErrorAction SilentlyContinue {output}\\results.jsonl",
                      capture_output=True, text=True, check=False).stdout.splitlines()
        for line in lines[len(results):]:
            result = json.loads(line)
            results.append(result)
            print(f"{'pass' if result['exit'] == 0 else 'FAIL'} {result['seconds']:5.0f}s {result['name']}",
                  flush=True)
    local = ROOT / "artifacts" / "windows-vm" / machine.name
    local.mkdir(parents=True, exist_ok=True)
    fetch = subprocess.Popen(ssh_command(machine, f"tar -cf - -C {GUEST_REPO}\\artifacts\\windows\\vm-fixtures {name}"),
                             stdout=subprocess.PIPE)
    run("tar", "-xf", "-", "-C", local, stdin=fetch.stdout)
    fetch.wait()
    failed = [result["name"] for result in results if result["exit"] != 0]
    print(f"{len(results) - len(failed)} passed, {len(failed)} failed. Logs: {local / name}")
    if not (local / name / "complete").exists():
        sys.exit("The fixture runner stopped before finishing.")
    if failed:
        sys.exit(1)


def screenshot(args):
    machine = selected(args)
    require_running(machine)
    qmp(machine, "screendump", filename=str(args.file.resolve()), format="png")


def reset(args):
    machine = selected(args)
    if running(machine):
        sys.exit(f"Stop the VM first with `{PROGRAM} --vm {machine.name} stop`.")
    remove(machine.directory)


def list_machines(args):
    for machine in machines():
        state = f"running, ssh port {machine.ssh_port.read_text().strip()}" if running(machine) else "stopped"
        print(f"{machine.name}\t{state}")
    if process(LEGACY_PID, "qemu-system-x86"):
        print("(unnamed VM from an older windows-vm.py)\trunning")


def destroy(args):
    active = [machine.name for machine in machines() + [installer()] if running(machine)]
    if process(LEGACY_PID, "qemu-system-x86"):
        active.append("(unnamed VM from an older windows-vm.py)")
    if active:
        sys.exit(f"Stop these VMs first: {', '.join(active)}")
    for path in (INSTALL, BASE, VMS, LEGACY_RUN, STATE / "run.partial", FIRMWARE, PASSWORD, KNOWN_HOSTS):
        remove(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--vm", help="VM name (default: $CAPYCANVAS_VM or the worktree directory name)")
    commands = parser.add_subparsers(required=True, metavar="command")

    def command(name, handler, summary):
        subcommand = commands.add_parser(name, help=summary, description=summary)
        subcommand.set_defaults(handler=handler)
        return subcommand

    command("setup", setup, "install host packages, grant KVM access and download the ISO").add_argument(
        "--iso", type=Path, help="use this ISO instead of downloading one")
    command("create", create, "install Windows and the build tools into the base image").add_argument(
        "--gui", action="store_true", help="show the installer display in a window")
    command("start", start, "boot the VM, creating it from the base image if needed").add_argument(
        "--gui", action="store_true", help="show the VM display in a window")
    command("stop", stop, "shut the VM down")
    command("list", list_machines, "list the VMs made from the base image")
    command("ssh", ssh, "open a PowerShell session or run a command in the VM").add_argument(
        "command", nargs=argparse.REMAINDER)
    command("sync", sync, f"copy the working tree to {GUEST_REPO}, keeping build outputs")
    command("check", check, "sync, build and run the tests that need no GPU").add_argument(
        "--release", action="store_true", help="build the Release configuration")
    fixture = command("fixtures", fixtures, "sync, build and run the UI fixtures on the software adapter")
    fixture.add_argument("names", nargs="*", help="fixtures to run, e.g. layers or header:pen (default: all)")
    fixture.add_argument("--no-build", action="store_true", help="reuse the last software-adapter build")
    command("screenshot", screenshot, "save the VM display as a PNG").add_argument("file", type=Path)
    command("reset", reset, "delete the VM; the next start makes a fresh one from the base image")
    command("destroy", destroy, "delete the base image and every VM, keeping the downloaded ISO")
    args = parser.parse_args()
    try:
        args.handler(args)
    except subprocess.CalledProcessError as error:
        sys.exit(f"Exit status {error.returncode}: {shlex.join(map(str, error.cmd))}")


if __name__ == "__main__":
    main()
