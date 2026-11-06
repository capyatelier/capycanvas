#!/usr/bin/env python3
"""Manage Windows 11 evaluation VMs that build the Windows client and run its tests.

See docs/development/windows-vm.md.
"""

import argparse
import base64
from contextlib import contextmanager, nullcontext, ExitStack
from concurrent.futures import ThreadPoolExecutor
import fcntl
import getpass
import hashlib
import json
import math
import os
from pathlib import Path
import re
import resource
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

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gpu_pool
import cpu_pool
import fixture_shards


ROOT = Path(__file__).resolve().parents[2]
GUEST_FILES = Path(__file__).resolve().parent
PROGRAM = sys.argv[0]
ISO_URL = "https://aka.ms/Win11E-ISO-25H2-en-us"
STATE = Path(os.environ.get("CAPYCANVAS_VM_DIR") or Path(
    os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share") / "capycanvas/windows-vm")
RUNTIME = Path(os.environ["CAPYCANVAS_VM_RUNTIME"]) if os.environ.get("CAPYCANVAS_VM_RUNTIME") else (
    Path(os.environ.get("XDG_RUNTIME_DIR") or tempfile.gettempdir()) /
    f"capycanvas-vm-{os.getuid()}-{hashlib.sha256(bytes(STATE)).hexdigest()[:8]}")
SETTINGS = gpu_pool.configuration(STATE)
CPUS = os.environ.get("CAPYCANVAS_VM_CPUS") or str(SETTINGS.get("cpus", min(8, os.cpu_count() or 1)))
MEMORY = os.environ.get("CAPYCANVAS_VM_MEMORY") or SETTINGS.get("memory", "16G")
ARTIFACTS = Path(os.environ.get("CAPYCANVAS_VM_ARTIFACTS") or SETTINGS.get("artifacts") or ROOT / "artifacts/windows-vm")
DISPLAY = os.environ.get("CAPYCANVAS_VM_DISPLAY") or "2560x1600"
VGA = "VGA,xres={},yres={},vgamem_mb=64".format(*DISPLAY.split("x"))
DISK_SIZE = "128G"
USER = "capy"
SEED_LABEL = "CAPYSEED"
GUEST_REPO = "C:\\capycanvas"
GUEST_PWSH = "C:\\Program Files\\PowerShell\\7\\pwsh.exe"
DESKTOP_TASK = "capycanvas-desktop"
SOFTWARE_BUILD = f"{GUEST_REPO}\\artifacts\\windows\\SoftwareAdapter"
HARDWARE_BUILD = f"{GUEST_REPO}\\artifacts\\windows\\Release"
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
        self.gpu = directory / "gpu"
        self.runner = directory / "runner.json"


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


def write_private(path, text):
    flags = os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW
    with os.fdopen(os.open(path, flags, 0o600), "w") as output:
        os.fchmod(output.fileno(), 0o600)
        output.truncate()
        output.write(text)


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
    STATE.chmod(0o700)
    if shutil.which("chattr"):
        subprocess.run(["chattr", "+C", STATE], stderr=subprocess.DEVNULL)
    ISO_RECORD.write_text(f"{iso}\n")
    print(f"Windows ISO: {iso}\nNext: {PROGRAM} create")


def process(pidfile, name):
    try:
        pid = int(pidfile.read_text())
        command = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
        owned = str(pidfile).encode() in command or f"file={pidfile}".encode() in command
        return pid if owned and Path(f"/proc/{pid}/comm").read_text().strip() == name else None
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
        return reply["return"]


def forwarded_ssh_port(machine):
    output = qmp(machine, "human-monitor-command", **{"command-line": "info usernet"})
    ports = re.findall(r"^\s*TCP\[HOST_FORWARD\]\s+\d+\s+127\.0\.0\.1\s+(\d+)\s+10\.0\.2\.15\s+22(?:\s|$)",
                       output or "", re.MULTILINE)
    if len(ports) != 1 or not 0 < int(ports[0]) <= 65535:
        sys.exit(f"QEMU did not report one bound SSH forwarding port for {machine.name}.")
    return int(ports[0])


def launch(machine, cdroms=(), gui=False, gpu=None, reservations=(), cpus=None, memory=None):
    for directory in (RUNTIME, machine.runtime):
        directory.mkdir(mode=0o700, exist_ok=True)
    terminate(machine.tpm_pid, "swtpm")
    selected_firmware = json.loads(FIRMWARE.read_text())
    machine.ssh_port.unlink(missing_ok=True)
    run("swtpm", "socket", "--tpm2", "--tpmstate", f"dir={machine.directory / 'tpm'}",
        "--ctrl", f"type=unixio,path={machine.tpm_socket}", "--pid", f"file={machine.tpm_pid}",
        "--terminate", "--daemon")
    command = [
        "qemu-system-x86_64", "-name", f"capycanvas-windows-{machine.name.lstrip('.')}", "-nodefaults",
        "-machine", "q35,accel=kvm,smm=on", "-global", "driver=cfi.pflash01,property=secure,value=on",
        "-cpu", f"host,{HYPERV}", "-smp", f"cpus={cpus or CPUS},sockets=1,cores={cpus or CPUS},threads=1",
        "-m", memory or MEMORY, "-rtc", "base=utc",
        "-drive", f"if=pflash,unit=0,readonly=on,format={selected_firmware['code_format']},"
                  f"file={selected_firmware['code']}",
        "-drive", f"if=pflash,unit=1,format={selected_firmware['vars_format']},file={machine.directory / 'efivars'}",
        "-chardev", f"socket,id=tpm,path={machine.tpm_socket}", "-tpmdev", "emulator,id=tpm,chardev=tpm",
        "-device", "tpm-crb,tpmdev=tpm",
        "-drive", f"if=none,id=disk,format=qcow2,discard=unmap,file={machine.directory / 'disk.qcow2'}",
        "-device", "nvme,drive=disk,serial=capycanvas,bootindex=0",
        "-netdev", "user,id=net,hostfwd=tcp:127.0.0.1:0-:22", "-device", "e1000e,netdev=net",
        "-device", "qemu-xhci", "-device", "usb-tablet", "-device", VGA,
        "-qmp", f"unix:{machine.qmp_socket},server=on,wait=off", "-display", "gtk" if gui else "none",
        "-daemonize", "-pidfile", machine.qemu_pid,
    ]
    for index, image in enumerate(cdroms):
        command += ["-drive", f"if=none,id=cd{index},media=cdrom,readonly=on,file={image}",
                    "-device", f"ide-cd,drive=cd{index},bus=ide.{index},bootindex={index + 1}"]
    if gpu:
        command += ["-global", "q35-pcihost.pci-hole64-size=256G",
                    "-fw_cfg", "name=opt/ovmf/X-PciMmio64Mb,string=262144",
                    "-device", "pcie-root-port,id=gpuroot,bus=pcie.0,addr=0x10,chassis=1"]
        for function in gpu.functions:
            slot = function.rsplit(".", 1)[1]
            command += ["-device", f"vfio-pci,host={function},bus=gpuroot,addr=0x0.{slot},multifunction=on"]
        machine.gpu.write_text(gpu.address + "\n")
    else:
        machine.gpu.unlink(missing_ok=True)
    kind = "gpu" if gpu else "runner"
    for index, reservation in enumerate(reservations):
        command += ["-add-fd", f"fd={reservation.fileno()},set={99 + index},opaque=capycanvas-{kind}-{machine.name}"]
    try:
        run(*command, **({"pass_fds": tuple(lock.fileno() for lock in reservations)} if reservations else {}))
    except subprocess.CalledProcessError:
        terminate(machine.tpm_pid, "swtpm")
        raise
    if machine.qemu_pid.exists():
        machine.launched_pid = int(machine.qemu_pid.read_text())
    machine.ssh_port.write_text(f"{forwarded_ssh_port(machine)}\n")


def ssh_command(machine, *remote):
    return ["ssh", "-i", str(KEY), "-p", machine.ssh_port.read_text().strip(), "-o", "IdentitiesOnly=yes",
            "-o", "BatchMode=yes", "-o", f"UserKnownHostsFile={KNOWN_HOSTS}",
            "-o", "StrictHostKeyChecking=accept-new", "-o", "HostKeyAlias=capycanvas-windows-vm",
            "-o", "ConnectTimeout=30", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=8",
            "-o", "LogLevel=ERROR", f"{USER}@127.0.0.1", *remote]


def guest(machine, command, **options):
    return run(*ssh_command(machine, command), **options)


def guest_script(machine, script, **options):
    encoded = base64.b64encode(script.encode("utf-16-le")).decode()
    payload = "\n".join(encoded[index:index + 1024] for index in range(0, len(encoded), 1024)) + "\n\n"
    loader = "$data=[Text.StringBuilder]::new(); while($line=[Console]::In.ReadLine()){$null=$data.Append($line)}; & ([ScriptBlock]::Create([Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($data.ToString()))))"
    command = base64.b64encode(loader.encode("utf-16-le")).decode()
    text = options.get("text") or options.get("universal_newlines") or options.get("encoding") or options.get("errors")
    status = guest(machine, f"$input | powershell -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand {command}; exit $LASTEXITCODE",
                   input=payload if text else payload.encode("ascii"), check=False, **options).returncode
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
    STATE.chmod(0o700)
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
    write_private(PASSWORD, f"{password}\n")
    unattend = (GUEST_FILES / "autounattend.xml").read_text().replace("@PASSWORD@", password)
    write_private(seed / "autounattend.xml", unattend)
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
    if not previous:
        sys.exit(f"Could not read the {machine.name} VM's boot time before restarting it.")
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


def boot(machine, gui, gpu=None):
    machine.runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    with open(machine.runtime / "boot-lock", "a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            sys.exit(f"The {machine.name} VM is already starting.")
        boot_locked(machine, gui, gpu)


def boot_locked(machine, gui, gpu):
    runner = getattr(machine, "runner_owner", None)
    if machine.runner.exists():
        runner = json.loads(machine.runner.read_text())
    if runner and (runner.get("worktree") != str(ROOT.resolve()) or gpu):
        sys.exit("CPU runners belong to their creating worktree and cannot claim a GPU.")
    if running(machine):
        if gpu and not machine.gpu.exists():
            sys.exit(f"The {machine.name} VM is running without a reserved GPU; stop it before using --gpu.")
        if gpu and gpu != "auto" and machine.gpu.read_text().strip() != gpu:
            sys.exit(f"The {machine.name} VM already has another GPU; stop it before changing cards.")
        if machine.gpu.exists():
            prepare_gpu_display(machine)
        return
    if not (BASE / "disk.qcow2").exists():
        sys.exit(f"The base image does not exist; run `{PROGRAM} create`.")
    if gpu and resource.getrlimit(resource.RLIMIT_MEMLOCK)[0] != resource.RLIM_INFINITY:
        sys.exit("GPU VMs require unlimited locked memory; use start --gpu after the host GPU setup.")
    with ExitStack() as reservations:
        device = None
        descriptors = ()
        cpus, memory = None, None
        if runner or gpu and os.environ.get("CAPYCANVAS_VM_ROUND_GPU") == "1":
            slot, descriptors = reservations.enter_context(cpu_pool.reserve(
                STATE, machine.name, ROOT, slot_count=1 if runner else 2))
            cpus, memory = slot.cpus, slot.memory
            if gpu and (cpus, memory) != (8, "16G"):
                raise ValueError("Round GPU requires two 4-vCPU / 8-GiB CPU slots")
        if gpu:
            device, card = reservations.enter_context(gpu_pool.reserve(STATE, machine.name, gpu))
            descriptors = (card, *descriptors)
        if not machine.directory.exists():
            fresh = machine.directory.with_name(f"{machine.name}.partial")
            if runner and fresh.exists():
                sys.exit(f"An unfinished runner disk exists at {fresh}; inspect it before starting again.")
            remove(fresh)
            shutil.copytree(BASE, fresh, ignore=shutil.ignore_patterns("disk.qcow2"))
            run("qemu-img", "create", "-q", "-f", "qcow2", "-F", "qcow2", "-b", BASE / "disk.qcow2",
                fresh / "disk.qcow2")
            fresh.rename(machine.directory)
        if runner:
            machine.runner.write_text(json.dumps(runner, indent=2))
        launch(machine, gui=gui, gpu=device, reservations=descriptors, cpus=cpus, memory=memory)
    wait_for_ssh(machine, 10 * 60)
    prepare(machine)
    if gpu:
        prepare_gpu_driver(machine)
        record = prepare_gpu_display(machine)
        if os.environ.get("CAPYCANVAS_VM_ROUND_GPU") == "1":
            record = prepare_gpu_display(machine, fixture_scale=150)
            (machine.directory / "fixture-scale-change.json").write_text(json.dumps(record, indent=2))
            if record.get("restart_required"):
                restart(machine)
            record = prepare_gpu_display(machine)
            validate_round_display(record)


def prepare_gpu_driver(machine):
    driver = gpu_pool.configuration(STATE).get("guest_driver")
    if not driver:
        print("Install the vendor's Windows GPU driver in this VM before hardware tests.")
        return
    driver = Path(driver)
    manifests = sorted(path for path in driver.rglob("*") if path.suffix.lower() in (".inf", ".cat"))
    if not manifests or not any(path.suffix.lower() == ".inf" for path in manifests):
        sys.exit(f"The configured Windows GPU driver bundle is unavailable: {driver}")
    digest = hashlib.sha256()
    for path in manifests:
        digest.update(str(path.relative_to(driver)).encode())
        digest.update(path.read_bytes())
    version = digest.hexdigest()
    stamp = machine.directory / "gpu-driver"
    if stamp.exists() and stamp.read_text().strip() == version:
        return
    destination = f"C:\\capycanvas-gpu-driver\\{version}"
    guest(machine, f"New-Item -ItemType Directory -Force '{destination}' | Out-Null")
    archive = subprocess.Popen(["tar", "-cf", "-", "-C", str(driver), "."], stdout=subprocess.PIPE)
    try:
        status = guest(machine, f"tar -xf - -C {destination}", stdin=archive.stdout, check=False)
    finally:
        archive.stdout.close()
        archive_status = archive.wait()
    if archive_status or status.returncode:
        sys.exit("Could not stage the Windows GPU driver bundle")
    guest_script(machine, "$ErrorActionPreference='Stop'\n" + (GUEST_FILES / "guest-gpu.ps1").read_text() +
                 f"\nInstall-WindowsGpuDriver '{destination}'\n")
    guest(machine, f"Remove-Item -LiteralPath '{destination}' -Recurse -Force")
    restart(machine)
    stamp.write_text(version + "\n")


def prepare_gpu_display(machine, fixture_scale=None):
    width, height = map(int, DISPLAY.split("x"))
    directory = r"C:\capycanvas-vm-display"
    receipt = directory + r"\receipt.json"
    scale = f" -FixtureScale {fixture_scale}" if fixture_scale else ""
    task = "capycanvas-hardware-display" + ("-fixture" if fixture_scale else "")
    trigger = "" if fixture_scale else " -Trigger $trigger"
    with claim(machine):
        guest_script(machine, f"""$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$task='{task}'
$receipt='{receipt}'
$deadline=(Get-Date).AddSeconds(120)
while (!(Get-Process explorer -IncludeUserName -ErrorAction SilentlyContinue | Where-Object UserName -Like '*\\{USER}')) {{
    if ((Get-Date) -ge $deadline) {{ throw 'The interactive desktop did not start' }}
    Start-Sleep 1
}}
while ((Get-ScheduledTask $task -ErrorAction SilentlyContinue).State -eq 'Running') {{
    if ((Get-Date) -ge $deadline) {{ throw 'Hardware display task is still running' }}
    Start-Sleep 1
}}
""", timeout=180)
        guest(machine, f"New-Item -ItemType Directory -Force '{directory}' | Out-Null")
        for name in ("hardware-display.cs", "hardware-display.ps1"):
            content = base64.b64encode((GUEST_FILES / name).read_bytes()).decode("ascii")
            guest_script(machine, f"[IO.File]::WriteAllBytes('{directory}\\{name}', "
                         f"[Convert]::FromBase64String('{content}'))")
        guest_script(machine, f"""$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$task='{task}'
$receipt='{receipt}'
$deadline=(Get-Date).AddSeconds(120)
Remove-Item -LiteralPath $receipt -Force -ErrorAction SilentlyContinue
$action=New-ScheduledTaskAction -Execute '{GUEST_PWSH}' -Argument '-NoProfile -WindowStyle Hidden -File {directory}\\hardware-display.ps1 -Width {width} -Height {height} -Receipt {receipt}{scale}'
$principal=New-ScheduledTaskPrincipal -UserId {USER} -LogonType Interactive -RunLevel Highest
$trigger=New-ScheduledTaskTrigger -AtLogOn -User {USER}
$settings=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 3) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask $task -Action $action -Principal $principal{trigger} -Settings $settings -Force | Out-Null
Start-ScheduledTask $task
while (!(Test-Path -LiteralPath $receipt)) {{
    if ((Get-Date) -ge $deadline) {{ throw 'Hardware display receipt timed out' }}
    Start-Sleep 1
}}
$result=Get-Content -LiteralPath $receipt -Raw|ConvertFrom-Json
if (!$result.success) {{ throw $result.error }}
$result|ConvertTo-Json -Depth 5
""", timeout=180)
        result = guest(machine, f"Get-Content -LiteralPath '{receipt}' -Raw", capture_output=True, text=True)
        record = json.loads(result.stdout)
        display = record.get("display", {})
        if not record.get("success") or not display.get("primary") or "NVIDIA" not in display.get("adapter", "") or (
                display.get("width"), display.get("height"), display.get("refreshHz")) != (width, height, 120):
            sys.exit("Hardware desktop receipt does not match the requested NVIDIA primary mode.")
        (machine.directory / "display.json").write_text(json.dumps(record, indent=2))
        return record


def start(args):
    machine = selected(args)
    gpu = getattr(args, "gpu", None)
    if gpu and not running(machine) and not os.environ.get("CAPYCANVAS_VM_GPU_SERVICE"):
        start_gpu_service(machine, args)
        return
    try:
        boot(machine, args.gui, gpu)
    except (Exception, SystemExit) as error:
        if not gpu or not os.environ.get("CAPYCANVAS_VM_GPU_SERVICE"):
            raise
        (machine.runtime / "startup-error").write_text(str(error))
        if not running(machine):
            raise
        print(f"GPU VM startup check failed: {error}; the VM remains running for diagnosis.", file=sys.stderr)
        return
    print(f"The {machine.name} VM is running. Connect with `{PROGRAM} --vm {machine.name} ssh`.")


def gpu_unit(machine):
    return f"capy-vm-{hashlib.sha256(bytes(STATE)).hexdigest()[:8]}-{machine.name}"


def start_gpu_service(machine, args):
    machine.runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    with open(machine.runtime / "service-lock", "a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            sys.exit(f"The {machine.name} GPU VM is already starting.")
        if running(machine):
            sys.exit(f"The {machine.name} VM started in another process; inspect it before retrying.")
        start_gpu_service_locked(machine, args)


def start_gpu_service_locked(machine, args):
    error = machine.runtime / "startup-error"
    error.unlink(missing_ok=True)
    unit = gpu_unit(machine)
    run("systemctl", "--user", "stop", unit, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    program = [sys.executable, str(Path(__file__).resolve()), "--vm", machine.name]
    command = ["systemd-run", "--user", f"--unit={unit}", "--collect",
               "--property=Type=oneshot", "--property=RemainAfterExit=yes",
               "--property=KillMode=process", "--property=LimitMEMLOCK=infinity",
               "--property=TimeoutStartSec=45min",
               "--property=TimeoutStopSec=6min",
               f"--property=ExecStop={shlex.join([item.replace('$', '$$') for item in [*program, 'stop']])}"]
    environment = {"CAPYCANVAS_VM_GPU_SERVICE": "1", "CAPYCANVAS_VM_DIR": str(STATE),
                   "CAPYCANVAS_VM_RUNTIME": str(RUNTIME), "CAPYCANVAS_VM_CPUS": CPUS,
                   "CAPYCANVAS_VM_MEMORY": MEMORY, "CAPYCANVAS_VM_DISPLAY": DISPLAY}
    if getattr(args, "round_gpu", False):
        environment["CAPYCANVAS_VM_ROUND_GPU"] = "1"
    command += [f"--setenv={key}={value}" for key, value in environment.items()]
    command += [*[item.replace("$", "$$") for item in program], "start", "--gpu", args.gpu]
    if args.gui:
        command.append("--gui")
    status = run(*command, check=False)
    if error.exists():
        sys.exit(error.read_text())
    if status.returncode:
        sys.exit(f"The {machine.name} GPU VM did not start; see `journalctl --user -u {unit}`.")


def stop_machine(machine):
    pid = process(machine.qemu_pid, "qemu-system-x86")
    if pid is None:
        return
    qmp(machine, "system_powerdown")
    if not wait_for_exit(pid, 5 * 60):
        if machine.gpu.exists():
            sys.exit("The GPU VM did not shut down cleanly; it and its GPU reservation remain running.")
        qmp(machine, "quit")
        if not wait_for_exit(pid, 30):
            raise RuntimeError(f"The {machine.name} VM is still running after shutdown; its reservation remains held.")


def stop(args):
    machine = selected(args)
    stop_machine(machine)
    if not os.environ.get("CAPYCANVAS_VM_GPU_SERVICE"):
        run("systemctl", "--user", "stop", gpu_unit(machine), check=False,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def gpus(args):
    rows = gpu_pool.describe(STATE)
    if not rows:
        print("No GPU pool configured; see docs/development/windows-vm.md.")
    for row in rows:
        owner = row["owner"]
        detail = f" ({owner['vm']} in {owner['state']})" if owner else ""
        print(f"{row['address']}\t{row['status']}{detail}")


def runner_machine(primary, index):
    suffix = f"-r{index}"
    prefix = primary.name
    if len(prefix + suffix) > 32:
        prefix = prefix[:23 - len(suffix)] + "-" + hashlib.sha256(primary.name.encode()).hexdigest()[:8]
    name = prefix + suffix
    machine = Machine(name, VMS / name)
    if machine.directory.exists() and not machine.runner.exists():
        sys.exit(f"The existing {name} VM is not an owned CPU runner.")
    machine.runner_owner = {"worktree": str(ROOT.resolve()), "primary": primary.name}
    return machine


def runners(args):
    if args.reset:
        primary = selected(args)
        owned = [machine for machine in machines() if machine.runner.exists()
                 and json.loads(machine.runner.read_text()) == {"worktree": str(ROOT.resolve()), "primary": primary.name}]
        with ExitStack() as locks:
            for machine in owned:
                machine.runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
                boot_lock = locks.enter_context(open(machine.runtime / "boot-lock", "a"))
                try:
                    fcntl.flock(boot_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    sys.exit(f"The {machine.name} runner is starting; stop it before resetting.")
                locks.enter_context(claim(machine))
                if running(machine) or runner_reserved(machine):
                    sys.exit(f"Stop the {machine.name} runner before resetting it.")
            for machine in owned:
                remove(machine.directory)
                print(f"Reset {machine.name}")
    settings, rows = cpu_pool.describe(STATE)
    print(f"CPU runner budget: {settings['vcpu_budget']} vCPU / {settings['memory_budget_gib']} GiB; "
          f"runner {settings['runner_cpus']} vCPU / {settings['runner_memory_gib']} GiB; "
          f"per owner {settings['per_owner_limit']}")
    for row in rows:
        owner = row["owner"]
        detail = f" {owner['vm']} in {owner['worktree']} since {owner['started']}" if owner else ""
        print(f"{row['slot']}\t{row['status']}{detail}")
    print(f"{sum(row['status'] == 'free' for row in rows)} free, "
          f"{sum(row['status'] == 'reserved' for row in rows)} reserved")


def runner_reserved(machine):
    return any(row["owner"] and row["owner"].get("vm") == machine.name
               and row["owner"].get("state") == str(STATE.resolve()) for row in cpu_pool.describe(STATE)[1])


def ssh(args):
    machine = selected(args)
    require_running(machine)
    os.execvp("ssh", ssh_command(machine, *args.command))


def claim(machine):
    directory = machine.directory.resolve()
    directory.parent.mkdir(parents=True, exist_ok=True)
    lock = cpu_pool.lock_file(directory.with_name("." + directory.name + ".lock"))
    if lock is None:
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
    return hashes


def sync(args):
    machine = selected(args)
    with claim(machine):
        sync_machine(machine)


def check(args):
    machine = selected(args)
    boot(machine, gui=False)
    lock = claim(machine)
    sync_machine(machine)
    configuration = "Debug" if args.debug else "Release"
    guest(machine, f"& '{GUEST_PWSH}' -NoProfile -File '{GUEST_REPO}\\apps\\layer-windows\\scripts\\test-without-gpu.ps1' "
                   f"-Configuration {configuration}")


def desktop(machine, arguments):
    guest_script(machine, f"""$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$deadline = (Get-Date).AddMinutes(2)
while (!(Get-Process explorer -IncludeUserName -ErrorAction SilentlyContinue | Where-Object UserName -Like '*\\{USER}')) {{
    if ((Get-Date) -ge $deadline) {{ throw 'The interactive desktop did not start.' }}
    Start-Sleep 2
}}
$action = New-ScheduledTaskAction -Execute conhost.exe -Argument '"{GUEST_PWSH}" -NoProfile -Sta -ExecutionPolicy Bypass {arguments}' -WorkingDirectory '{GUEST_REPO}'
$principal = New-ScheduledTaskPrincipal -UserId {USER} -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit 0
Register-ScheduledTask {DESKTOP_TASK} -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask {DESKTOP_TASK}
""", timeout=180)


def desktop_running(machine):
    state = guest(machine, f"$ErrorActionPreference='Stop'; (Get-ScheduledTask {DESKTOP_TASK}).State",
                  capture_output=True, text=True, timeout=60)
    return state.stdout.strip() in ("Running", "Queued")


def build_inputs(hashes):
    scripts = {"apps/layer-windows/scripts/build.ps1", "apps/layer-windows/scripts/stage-assets.ps1",
               "apps/layer-windows/scripts/WindowsArchitecture.ps1"}
    return {name: digest for name, digest in hashes.items()
            if name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml") or name in scripts
            or name.startswith(("crates/", "vendor/", ".cargo/")) and not name.endswith(".md")
            or name.startswith(("apps/layer-web/icons/", "apps/layer-web/brush-previews/",
                                "apps/layer-web/filter-previews/", "assets/locales/"))
            or name.startswith("apps/layer-windows/") and not name.endswith((".md", ".ps1"))
            and not name.startswith("apps/layer-windows/scripts/")}


def build_files(machine, directory=SOFTWARE_BUILD):
    result = guest(machine, f"$ErrorActionPreference='Stop'; $build='{directory}'; "
                   "ConvertTo-Json -Compress -InputObject @(Get-ChildItem $build -Recurse -File | "
                   "Where-Object { $_.Extension -In '.exe','.dll','.pri','.winmd','.manifest','.xbf','.mui','.xml','.dat' "
                   "-or $_.Name -match '\\.(deps|runtimeconfig)\\.json$' -or $_.FullName.StartsWith($build+'\\Assets\\') } | "
                   "ForEach-Object { @{name=$_.FullName.Substring($build.Length+1);"
                   "sha256=(Get-FileHash -LiteralPath $_.FullName).Hash} })",
                   capture_output=True, text=True, timeout=60)
    return {item["name"]: item["sha256"] for item in json.loads(result.stdout)}


def validate_build(record, inputs, files, profile="release"):
    if not record or record.get("profile") != profile or record["inputs"] != inputs or record["files"] != files:
        sys.exit("The build does not match the synced source or binaries; rerun without --no-build.")


def build_record_path(machine, hardware, profile):
    suffix = "" if profile == "release" else f"-{profile}"
    return machine.directory / f"{'hardware' if hardware else 'software'}-build{suffix}.json"


def publish_build_record(machine, directory, record):
    payload = base64.b64encode(json.dumps(record).encode()).decode()
    guest_script(machine, f"[IO.File]::WriteAllBytes('{directory}\\fixture-build.json',"
                 f"[Convert]::FromBase64String('{payload}'))")


def validate_controls(machine, directory, record, hardware, required):
    controls = record.get("controls", {})
    if not set(required) <= set(controls) or record.get("hardware_gpu") != hardware:
        sys.exit("Missing control-fixture build record; rerun without --no-build.")
    for kind in required:
        files = controls[kind]
        if not files or files != build_files(machine, directory + "-" + kind):
            sys.exit(f"The {kind} control fixture does not match its build record; rerun without --no-build.")


def fixture_directory(hardware, profile):
    return (HARDWARE_BUILD if hardware else SOFTWARE_BUILD) + ("" if profile == "release" else f"-{profile}")


def fixture_build(machine, hashes, no_build, hardware, profile="release", controls=None):
    inputs = build_inputs(hashes)
    directory = fixture_directory(hardware, profile)
    intermediate = (HARDWARE_BUILD if hardware else SOFTWARE_BUILD) + ".obj"
    build_record = build_record_path(machine, hardware, profile)
    if no_build:
        record = json.loads(build_record.read_text()) if build_record.exists() else None
        validate_build(record, inputs, build_files(machine, directory), profile)
        if controls is not None:
            validate_controls(machine, directory, record, hardware, controls)
    else:
        build_record.unlink(missing_ok=True)
        feature = "" if hardware else "-SoftwareAdapterTests "
        guest(machine, f"& '{GUEST_PWSH}' -NoProfile -File '{GUEST_REPO}\\apps\\layer-windows\\scripts\\build.ps1' -Configuration Release "
                       f"-RustProfile {profile} {feature}-OutputDirectory '{directory}' -IntermediateDirectory '{intermediate}'")
        record = {"profile": profile, "inputs": inputs, "files": build_files(machine, directory)}
        if controls is not None:
            record.update(controls={}, hardware_gpu=hardware)
            for kind in controls:
                target = directory + "-" + kind
                guest(machine, f"& '{GUEST_PWSH}' -NoProfile -File '{GUEST_REPO}\\apps\\layer-windows\\scripts\\build.ps1' "
                      f"-Configuration Release -RustProfile {profile} {feature}-SkipRust -SkipRestore -ControlFixture {kind} "
                      f"-OutputDirectory '{target}' -IntermediateDirectory '{intermediate}'")
                record["controls"][kind] = build_files(machine, target)
        build_record.write_text(json.dumps(record))
    if controls is not None:
        record = dict(record, controls={kind: record["controls"][kind] for kind in controls})
        publish_build_record(machine, directory, record)
    return directory, record


def retrieve(machine, root, name):
    local = ARTIFACTS / machine.name
    local.mkdir(parents=True, exist_ok=True)
    fetch = subprocess.Popen(ssh_command(machine, f"tar -cf - -C {root} {name}"), stdout=subprocess.PIPE)
    run("tar", "-xf", "-", "-C", local, stdin=fetch.stdout)
    fetch.stdout.close()
    if fetch.wait():
        sys.exit("Could not retrieve the VM test evidence.")
    return local / name


def gpu_check(args):
    start(argparse.Namespace(vm=args.vm, gui=False, gpu="auto"))
    machine = selected(args)
    with claim(machine):
        revision = run("git", "rev-parse", "HEAD", cwd=ROOT, capture_output=True, text=True).stdout.strip()
        hashes = sync_machine(machine)
        directory, record = fixture_build(machine, hashes, args.no_build, True)
        name = "gpu-" + time.strftime("%Y%m%d-%H%M%S")
        root = f"{GUEST_REPO}\\artifacts\\windows\\vm-gpu-tests"
        status = guest(machine, f"& '{GUEST_PWSH}' -NoProfile -File '{GUEST_REPO}\\tools\\windows-vm\\test-with-gpu.ps1' "
                       f"-Configuration Release -Output '{root}\\{name}' -BuildDirectory '{directory}'", check=False)
        local = retrieve(machine, root, name)
        (local / "provenance.json").write_text(json.dumps({"revision": revision, "source": hashes, "build": record}, indent=2))
        print(f"Hardware GPU test evidence: {local}")
        if status.returncode:
            sys.exit(f"Hardware GPU tests failed (exit {status.returncode}); see {local}")


def validate_results(plan, results):
    expected = plan["runs"]
    actual = [row["name"] for row in results]
    if not expected or actual != expected:
        sys.exit(f"Incomplete fixture coverage: expected {expected}, received {actual}.")


def fixture_status_command(output):
    return r"""$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
function Read-FixtureText([string]$Path){
    $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader=[IO.StreamReader]::new($stream)
    try{$reader.ReadToEnd()}finally{$reader.Dispose();$stream.Dispose()}
}
@{plan=$(if(Test-Path '{output}\plan.json'){Read-FixtureText '{output}\plan.json'|ConvertFrom-Json});
log=$(if(Test-Path '{output}\results.jsonl'){Read-FixtureText '{output}\results.jsonl'});
error=$(if(Test-Path '{output}\runner-error.txt'){Read-FixtureText '{output}\runner-error.txt'});
complete=(Test-Path '{output}\complete')}|ConvertTo-Json -Depth 5 -Compress
""".replace("{output}", output)


def wait_for_fixtures(machine, output):
    results = []
    deadline = time.monotonic() + 180
    plan = None
    failure = None
    try:
        while True:
            time.sleep(5)
            running = desktop_running(machine)
            status = guest(machine, fixture_status_command(output), capture_output=True, text=True, timeout=60)
            try:
                progress = json.loads(status.stdout)
            except json.JSONDecodeError as error:
                raise RuntimeError(f"Invalid fixture progress JSON: {error}; stdout={status.stdout!r}; stderr={status.stderr!r}") from error
            if progress["plan"]:
                if plan is None:
                    deadline = time.monotonic() + progress["plan"]["timeout_minutes"] * 60 + 120
                plan = progress["plan"]
            for line in (progress["log"] or "").splitlines()[len(results):]:
                try:
                    result = json.loads(line)
                except json.JSONDecodeError:
                    break
                results.append(result)
                deadline = time.monotonic() + plan["timeout_minutes"] * 60 + 120
                print(f"{'pass' if result['exit'] == 0 else 'FAIL'} {result['seconds']:5.0f}s {result['name']}", flush=True)
            if progress["complete"]:
                break
            if time.monotonic() >= deadline:
                raise TimeoutError("The fixture runner exceeded its startup or fixture deadline.")
            if not running:
                raise RuntimeError(progress.get("error") or "The desktop task stopped before completing the fixture plan.")
    except (RuntimeError, TimeoutError, ValueError, TypeError, subprocess.SubprocessError) as error:
        failure = str(error)
        if isinstance(error, subprocess.CalledProcessError):
            failure += f"; stdout={error.stdout!r}; stderr={error.stderr!r}"
        guest(machine, f"Stop-ScheduledTask {DESKTOP_TASK}", check=False, timeout=60)
    return results, plan, failure


def fixture_selection(names):
    if any(not re.fullmatch(r"[a-zA-Z0-9:-]+", item) for item in names):
        raise ValueError("Fixture names use letters, numbers, hyphens and a variant colon.")
    return f" -Name {','.join(names)}" if names else ""


def execute_fixtures(machine, args, directory, provenance, names, queue=None):
    name = time.strftime("%Y%m%d-%H%M%S")
    output = f"{GUEST_REPO}\\artifacts\\windows\\vm-fixtures\\{name}"
    selection = fixture_selection(names)
    mode = " -HardwareGpu" if provenance["hardware_gpu"] else ""
    mode += f" -QueueConfig {queue}" if queue else ""
    desktop(machine, f"-File {GUEST_REPO}\\tools\\windows-vm\\run-fixtures.ps1 "
                     f"-Executable {directory}\\CapyCanvas.exe -Output {output} "
                     f"-TimeoutMinutes {args.timeout_minutes} -RustProfile {args.profile} "
                     f"-BuildRecord {directory}\\fixture-build.json{selection}{mode}")
    results, plan, failure = wait_for_fixtures(machine, output)
    local = retrieve(machine, f"{GUEST_REPO}\\artifacts\\windows\\vm-fixtures", name)
    (local / "provenance.json").write_text(json.dumps(provenance, indent=2))
    failed = [result["name"] for result in results if result["exit"] != 0]
    print(f"{len(results) - len(failed)} passed, {len(failed)} failed. Logs: {local}")
    return {"vm": machine.name, "results": results, "plan": plan, "failure": failure,
            "complete": (local / "complete").exists(), "evidence": str(local)}


def fixtures(args):
    if getattr(args, "keep_runners", False) and getattr(args, "runners", None) is None:
        raise ValueError("--keep-runners requires --runners")
    if not getattr(args, "hardware_gpu", False):
        return sharded_fixtures(args)
    if getattr(args, "runners", None) is not None:
        raise ValueError("CPU runners cannot be used with --hardware-gpu")
    fixture_selection(args.names)
    machine = selected(args)
    start(argparse.Namespace(vm=args.vm, gui=False, gpu="auto"))
    with claim(machine):
        revision = run("git", "rev-parse", "HEAD", cwd=ROOT, capture_output=True, text=True).stdout.strip()
        hashes = sync_machine(machine)
        plan = planned_fixtures(machine, args, fixture_directory(True, args.profile))
        directory, record = fixture_build(machine, hashes, args.no_build, True, args.profile, controls=plan["controls"])
        result = execute_fixtures(machine, args, directory,
                                  {"revision": revision, "source": hashes, "build": record, "hardware_gpu": True}, args.names)
    if result["failure"] or not result["complete"]:
        sys.exit(result["failure"] or "The fixture runner stopped before finishing.")
    validate_results(result["plan"], result["results"])
    if any(row["exit"] != 0 for row in result["results"]):
        sys.exit(1)


def planned_fixtures(machine, args, directory):
    result = guest(machine, f"& '{GUEST_PWSH}' -NoProfile -File '{GUEST_REPO}\\tools\\windows-vm\\run-fixtures.ps1' "
                   f"-Executable '{directory}\\CapyCanvas.exe' -Output '{directory}' -List"
                   + fixture_selection(args.names), capture_output=True, text=True)
    plan = json.loads(result.stdout)
    selectors = plan["runs"]
    if not isinstance(selectors, list) or not selectors or len(set(selectors)) != len(selectors):
        raise ValueError("The guest returned an empty or duplicate fixture plan")
    fixture_selection(selectors)
    requirements = plan.get("requirements")
    if not isinstance(requirements, dict) or set(requirements) != set(selectors):
        raise ValueError("Fixture requirements do not cover the exact plan")
    for row in requirements.values():
        if not isinstance(row, dict) or type(row.get("hardware_gpu")) is not bool or not isinstance(row.get("controls"), list) or any(kind not in ("Feedback", "Popup", "LayerDrop") for kind in row["controls"]):
            raise ValueError("Invalid fixture adapter or control requirements")
    return plan


def fixture_durations():
    durations = {}
    roots = {ARTIFACTS, Path(SETTINGS.get("artifacts") or ARTIFACTS)}
    files = sorted({path for root in roots for path in root.glob("*/*/results.jsonl")},
                   key=lambda path: path.stat().st_mtime)
    for path in files:
        for line in path.read_text(encoding="utf-8-sig").splitlines():
            try:
                row = json.loads(line)
                if type(row["seconds"]) in (int, float) and math.isfinite(row["seconds"]) and row["seconds"] > 0:
                    durations[row["name"]] = row["seconds"]
            except (ValueError, KeyError):
                continue
    return durations


def fixture_roots(directory, record):
    root = directory.rsplit("\\", 1)[-1]
    return [root, *[root + "-" + kind for kind in record["controls"]]]


def export_fixture_build(machine, directory, record, local):
    archive = local / "build.tar.gz"
    roots = " ".join(fixture_roots(directory, record))
    with archive.open("wb") as stream:
        run(*ssh_command(machine, f"tar -czf - -C {GUEST_REPO}\\artifacts\\windows {roots}"), stdout=stream)
    return archive


def import_fixture_build(machine, directory, record, archive, hashes, profile):
    for name in fixture_roots(directory, record):
        target = f"{GUEST_REPO}\\artifacts\\windows\\{name}"
        guest(machine, f"if(Test-Path -LiteralPath '{target}'){{Remove-Item -LiteralPath '{target}' -Recurse -Force}}")
    build_record_path(machine, False, profile).unlink(missing_ok=True)
    guest(machine, f"New-Item -ItemType Directory -Force '{GUEST_REPO}\\artifacts\\windows'|Out-Null")
    with archive.open("rb") as stream:
        run(*ssh_command(machine, f"tar -xzmf - -C {GUEST_REPO}\\artifacts\\windows"), stdin=stream)
    validate_build(record, build_inputs(hashes), build_files(machine, directory), profile)
    validate_controls(machine, directory, record, False, record["controls"])
    publish_build_record(machine, directory, record)
    build_record_path(machine, False, profile).write_text(json.dumps(record))


@contextmanager
def fixture_queue_config(machine, queue):
    path = f"{GUEST_REPO}\\artifacts\\windows\\fixture-queue-{secrets.token_hex(16)}.json"
    try:
        payload = json.dumps(queue.configuration(machine.name))
        guest(machine, f"[IO.File]::WriteAllText('{path}', [Console]::In.ReadToEnd())",
              input=payload, text=True, stdout=subprocess.DEVNULL)
        yield path
    finally:
        guest(machine, f"if(Test-Path -LiteralPath '{path}'){{Remove-Item -LiteralPath '{path}' -Force -ErrorAction Stop}}")


def validate_round_display(record):
    display = record.get("display", {})
    if record.get("restart_required") or not record.get("success") or not display.get("primary") or "NVIDIA" not in display.get("adapter", "") or display.get("refreshHz") != 120 or (
            display.get("width"), display.get("height"), display.get("scalePercent")) != (2560, 1600, 150):
        raise ValueError("Round GPU requires a verified 2560x1600 desktop at 150% scale")


def fixture_memory_gib(memory):
    size = re.fullmatch(r"([1-9][0-9]*)([KMGT]?)", str(memory), re.IGNORECASE)
    if not size:
        raise ValueError("Fixture VM memory must be a positive integer with an optional K, M, G or T suffix")
    power = {"K": 10, "M": 20, "": 20, "G": 30, "T": 40}[size[2].upper()]
    return ((int(size[1]) << power) + (1 << 30) - 1) // (1 << 30)


def fixture_memory_admission(local, phase, sizes):
    available = next(int(line.split()[1]) * 1024 for line in Path("/proc/meminfo").read_text().splitlines()
                     if line.startswith("MemAvailable:"))
    required = (sum(sizes) + 40) * 1024 ** 3
    receipt = {"phase": phase, "available_bytes": available, "required_bytes": required,
               "vm_sizes_gib": sizes, "admitted": available >= required}
    with (local / "memory-admission.jsonl").open("a") as output:
        output.write(json.dumps(receipt) + "\n")
    if not receipt["admitted"]:
        raise RuntimeError(f"{phase}: MemAvailable {available} bytes is below required {required}")


def sharded_fixtures(args):
    if args.hardware_gpu:
        raise ValueError("CPU runners cannot be used with --hardware-gpu")
    fixture_selection(args.names)
    primary = selected(args)
    if primary.runner.exists():
        raise ValueError("Build and shard from the worktree's primary VM")
    local = ARTIFACTS / primary.name / ("shards-" + time.strftime("%Y%m%d-%H%M%S"))
    local.mkdir(parents=True, exist_ok=False)
    started = time.monotonic()
    used, locks, shards = [], {}, []
    selectors, cpu_selectors, gpu_selectors = [], [], []
    provenance = {}
    primary_owned = False
    warm_cpu = False
    def exact_names(names):
        return [name if ":" in name else name + ":default" for name in names]
    def start_runner(machine):
        lock = claim(machine)
        try:
            boot(machine, gui=False)
            return machine, lock, None
        except (Exception, SystemExit) as error:
            try:
                if getattr(machine, "launched_pid", None) is not None and machine.qemu_pid.exists() and int(machine.qemu_pid.read_text()) == machine.launched_pid:
                    stop_machine(machine)
            finally:
                lock.close()
            if isinstance(error, cpu_pool.Unavailable):
                return None, None, str(error)
            raise
    def run_cpu(machine):
        result = {"vm": machine.name, "results": [], "complete": False, "failure": None}
        try:
            directory, record = builds[False]
            if machine is not primary:
                synced = sync_machine(machine)
                if synced != hashes:
                    raise ValueError("The source tree changed while preparing fixture runners")
                import_fixture_build(machine, directory, record, archive, synced, args.profile)
            with (fixture_queue_config(machine, queue) if queue else nullcontext(None)) as configuration:
                result = execute_fixtures(machine, args, directory, mode_provenance(False),
                                          exact_names(cpu_selectors), queue=configuration)
        except (Exception, SystemExit) as error:
            result["failure"] = "; ".join(filter(None, (result.get("failure"), str(error))))
        return dict(result, selectors=queue.assignments[machine.name] if queue else cpu_selectors)

    def mode_provenance(hardware):
        return {"revision": provenance["revision"], "source": hashes,
                "build": builds[hardware][1], "hardware_gpu": hardware}
    def run_gpu():
        nonlocal primary_owned
        result = {"vm": primary.name, "results": [], "complete": False, "failure": None}
        try:
            with claim(primary):
                try:
                    if sync_machine(primary) != hashes:
                        raise ValueError("The source tree changed while preparing GPU fixtures")
                    directory, expected = builds[True]
                    directory, record = fixture_build(primary, hashes, True, True, args.profile,
                                                       controls=list(expected["controls"]))
                    if record != expected:
                        raise ValueError("The retained hardware build changed before GPU execution")
                    publish_build_record(primary, directory, dict(record, fixture_scale=1.5))
                    result = execute_fixtures(primary, args, directory, mode_provenance(True), exact_names(gpu_selectors))
                finally:
                    try:
                        stop_machine(primary)
                        primary_owned = False
                    except (Exception, SystemExit) as error:
                        result["failure"] = "; ".join(filter(None, (result.get("failure"), f"GPU shutdown: {error}")))
        except (Exception, SystemExit) as error:
            result["failure"] = "; ".join(filter(None, (result.get("failure"), str(error))))
        return dict(result, selectors=gpu_selectors)
    try:
        with claim(primary):
            if primary.gpu.exists() and running(primary):
                raise ValueError("Stop the primary GPU VM before preparing a fixture round")
            if not running(primary):
                fixture_memory_admission(local, "build-primary", [fixture_memory_gib(MEMORY)])
            primary_owned = True
            boot(primary, gui=False)
            hashes = sync_machine(primary)
            plan = dict(planned_fixtures(primary, args, fixture_directory(False, args.profile)))
            selectors = plan["runs"]
            requirements = plan["requirements"]
            cpu_selectors = [name for name in selectors if not requirements[name]["hardware_gpu"]]
            gpu_selectors = [name for name in selectors if requirements[name]["hardware_gpu"]]
            warm_cpu = args.runners is None and bool(args.names) and not gpu_selectors
            primary_owned = not warm_cpu
            (local / "plan.json").write_text(json.dumps(plan, indent=2))
            if gpu_selectors and (str(CPUS), MEMORY, DISPLAY) != ("8", "16G", "2560x1600"):
                raise ValueError("Round GPU requires existing 8-vCPU / 16-GiB, 2560x1600 settings")
            builds = {}
            for hardware, names in ((False, cpu_selectors), (True, gpu_selectors)):
                if names:
                    controls = sorted({kind for name in names for kind in requirements[name]["controls"]})
                    builds[hardware] = fixture_build(primary, hashes, args.no_build, hardware, args.profile, controls=controls)
            provenance = {"revision": run("git", "rev-parse", "HEAD", cwd=ROOT, capture_output=True, text=True).stdout.strip(),
                          "source": hashes, "build": builds[False if cpu_selectors else True][1], "hardware_gpu": not bool(cpu_selectors),
                          "builds": {"hardware" if mode else "software": record for mode, (_, record) in builds.items()}}
            (local / "provenance.json").write_text(json.dumps(provenance, indent=2))
            if warm_cpu:
                queue = None
                shards = [run_cpu(primary)]
            else:
                archive = export_fixture_build(primary, *builds[False], local) if cpu_selectors else None
                stop_machine(primary)
                primary_owned = False
        if gpu_selectors:
            fixture_memory_admission(local, "gpu-primary", [16])
            primary_owned = True
            start(argparse.Namespace(vm=primary.name, gui=False, gpu="auto", round_gpu=True))
            display = json.loads((primary.directory / "display.json").read_text())
            (local / "gpu-display.json").write_text(json.dumps(display, indent=2))
            shutil.copyfile(primary.directory / "fixture-scale-change.json", local / "gpu-scale-change.json")
            validate_round_display(display)
        if cpu_selectors and not warm_cpu:
            settings, slots = cpu_pool.describe(STATE)
            free = sum(row["status"] == "free" for row in slots)
            owned = sum(bool(row["owner"]) and row["owner"].get("worktree") == str(ROOT.resolve()) for row in slots)
            limit = min(args.runners or 1, len(cpu_selectors), settings["per_owner_limit"] - (2 if gpu_selectors else 0))
            candidates = [runner_machine(primary, index + 1) for index in range(limit)]
            retained = [machine for machine in candidates if running(machine)]
            new = [machine for machine in candidates if machine not in retained]
            count = min(free, max(0, settings["per_owner_limit"] - owned))
            candidates = retained + new[:count]
            if not candidates:
                raise RuntimeError("No CPU runner slot was available; see `runners`")
            fixture_memory_admission(local, "cpu-batch", [settings["runner_memory_gib"]] * min(count, len(new)))
            count = len(candidates)
            with ThreadPoolExecutor(max_workers=count) as executor:
                futures = [executor.submit(start_runner, machine) for machine in candidates]
                failures = []
                for future in futures:
                    try:
                        machine, lock, error = future.result()
                        if machine:
                            used.append(machine)
                            locks[machine.name] = lock
                        else:
                            print(error, flush=True)
                    except (Exception, SystemExit) as error:
                        failures.append(str(error))
                if failures:
                    raise RuntimeError("; ".join(failures))
            if not used:
                raise RuntimeError("No CPU runner slot was available; see `runners`")
        if not warm_cpu:
            with (fixture_shards.Queue(cpu_selectors, [machine.name for machine in used], fixture_durations(), local / "cpu-plan.json")
                  if used else nullcontext(None)) as queue:
                with ThreadPoolExecutor(max_workers=len(used) + bool(gpu_selectors)) as executor:
                    futures = [executor.submit(run_cpu, machine) for machine in used]
                    if gpu_selectors:
                        futures.append(executor.submit(run_gpu))
                    shards = [future.result() for future in futures]
    except (Exception, SystemExit) as error:
        shards.append({"vm": primary.name + "-setup", "selectors": [], "results": [],
                       "complete": False, "failure": str(error)})
    finally:
        cleanup_errors = []
        for machine in [*used, *([primary] if primary_owned else [])]:
            try:
                if machine is primary or not args.keep_runners:
                    stop_machine(machine)
            except (Exception, SystemExit) as error:
                cleanup_errors.append(f"{machine.name}: {error}")
            finally:
                if machine.name in locks:
                    locks[machine.name].close()
        if selectors:
            results, errors = fixture_shards.merge(selectors, shards)
        else:
            results, errors = [], ["No fixture plan was prepared", *[row["failure"] for row in shards if row.get("failure")]]
        errors.extend(cleanup_errors)
        if selectors:
            plan["shards"] = {row["vm"]: row["selectors"] for row in shards}
            (local / "plan.json").write_text(json.dumps(plan, indent=2))
        (local / "results.jsonl").write_text("".join(json.dumps(row) + "\n" for row in results))
        (local / "shards.json").write_text(json.dumps(shards, indent=2))
        (local / "summary.json").write_text(json.dumps({"wall_seconds": time.monotonic() - started, "errors": errors}, indent=2))
        if cleanup_errors:
            (local / "shutdown-errors.json").write_text(json.dumps(cleanup_errors, indent=2))
    destination = "the primary VM" if warm_cpu else f"{len(used)} CPU runners and {int(bool(gpu_selectors))} GPU VM"
    print(f"{sum(row['exit'] == 0 for row in results)}/{len(selectors)} passed on {destination}. Logs: {local}")
    if errors:
        raise RuntimeError("; ".join(errors))
    (local / "complete").touch()


def screenshot(args):
    machine = selected(args)
    require_running(machine)
    qmp(machine, "screendump", filename=str(args.file.resolve()), format="png")


def reset(args):
    machine = selected(args)
    if machine.runner.exists() and json.loads(machine.runner.read_text()).get("worktree") != str(ROOT.resolve()):
        sys.exit(f"The {machine.name} runner belongs to another worktree.")
    if running(machine) or machine.runner.exists() and runner_reserved(machine):
        sys.exit(f"Stop the VM first with `{PROGRAM} --vm {machine.name} stop`.")
    remove(machine.directory)


def list_machines(args):
    for machine in machines():
        state = f"running, ssh port {machine.ssh_port.read_text().strip()}" if running(machine) else "stopped"
        if state != "stopped" and machine.gpu.exists():
            state += f", GPU {machine.gpu.read_text().strip()}"
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


def positive_int(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("Expected a positive integer")
    return number


def fixture_timeout(value):
    minutes = int(value)
    if not 1 <= minutes <= 1440:
        raise argparse.ArgumentTypeError("Fixture timeout must be 1–1440 minutes.")
    return minutes


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
    start_command = command("start", start, "boot the VM, creating it from the base image if needed")
    start_command.add_argument("--gui", action="store_true", help="show the VM display in a window")
    start_command.add_argument("--gpu", nargs="?", const="auto", help="claim one free GPU, or the specified pool PCI address")
    command("gpus", gpus, "show the hardware GPU pool and its reservations")
    command("runners", runners, "show CPU runner reservations").add_argument(
        "--reset", action="store_true", help="delete this owner's stopped runner disks")
    command("gpu-check", gpu_check, "run the hardware renderer and Windows D3D12 tests").add_argument(
        "--no-build", action="store_true", help="reuse a validated hardware app build")
    command("stop", stop, "shut the VM down")
    command("list", list_machines, "list the VMs made from the base image")
    command("ssh", ssh, "open a PowerShell session or run a command in the VM").add_argument(
        "command", nargs=argparse.REMAINDER)
    command("sync", sync, f"copy the working tree to {GUEST_REPO}, keeping build outputs")
    command("check", check, "sync, build and run the tests that need no GPU").add_argument(
        "--debug", action="store_true", help="use unoptimized Debug builds and tests (default: Release)")
    fixture = command("fixtures", fixtures, "sync, build and run UI fixtures (no names: one CPU runner plus one GPU VM)")
    fixture.add_argument("names", nargs="*", help="fixtures to run, e.g. layers or header:pen (default: all)")
    fixture.add_argument("--no-build", action="store_true", help="reuse a validated build for the selected adapter mode")
    fixture.add_argument("--profile", choices=("release", "dev-perf"), default="release",
                         help="Rust build profile (default: release; dev-perf enables incremental development builds)")
    fixture.add_argument("--hardware-gpu", action="store_true", help="claim a GPU and test a hardware-only app build")
    fixture.add_argument("--runners", type=positive_int, help="run WARP fixtures on up to N pooled CPU VMs")
    fixture.add_argument("--keep-runners", action="store_true", help="retain CPU runners and their reservations after a sharded run")
    fixture.add_argument("--timeout-minutes", type=fixture_timeout, default=20,
                         help="deadline for each fixture, 1–1440 minutes (default: 20)")
    command("screenshot", screenshot, "save the VM display as a PNG").add_argument("file", type=Path)
    command("reset", reset, "delete the VM; the next start makes a fresh one from the base image")
    command("destroy", destroy, "delete the base image and every VM, keeping the downloaded ISO")
    args = parser.parse_args()
    try:
        args.handler(args)
    except (OSError, ValueError, RuntimeError) as error:
        sys.exit(str(error))
    except subprocess.CalledProcessError as error:
        sys.exit(f"Exit status {error.returncode}: {shlex.join(map(str, error.cmd))}")


if __name__ == "__main__":
    main()
