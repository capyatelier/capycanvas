#!/usr/bin/env python3
"""Isolated test-only tablet-v2 input through GTK's actual Wayland backend.

Mutter RemoteDesktop supplies mouse/touch but no tablet. This transparent proxy
adds a tablet/tool to the test client's existing seat. It never connects to the
user's display. Synthetic tablet serials cannot authorize compositor DND; use
the existing Mutter journeys for that, and this fixture for panel contacts.
"""
import array
import json
import os
import selectors
import socket
import struct
import sys
import time
from pathlib import Path

runtime = Path(os.environ["XDG_RUNTIME_DIR"])
display = os.environ["WAYLAND_DISPLAY"]
if not display.startswith("layer-bench-"):
    raise SystemExit("Requires the isolated native-input runner")
path = runtime / "layer-bench-tablet"
events = Path(os.environ["LAYER_NATIVE_INPUT_DIR"])
tablet, tool = 0xFF000000, 0xFF000001
objects = {1: "wl_display"}
surfaces = {}
target = None
seat = None
introduced = False
near = False
serial = 10000


def words(*values):
    return struct.pack("=" + "I" * len(values), *values)


def string(value):
    value = value.encode() + b"\0"
    return words(len(value)) + value + bytes((-len(value)) % 4)


def event(obj, opcode, payload=b""):
    client.sendall(words(obj, ((len(payload) + 8) << 16) | opcode) + payload)


def request(message):
    global seat, target
    obj, header = struct.unpack_from("=II", message)
    opcode = header & 0xFFFF
    args = message[8:]
    if obj in (tablet, tool):
        return False  # Gtk's cursor/destroy requests for our synthetic objects.
    kind = objects.get(obj)
    if kind == "wl_display" and opcode == 1:
        objects[struct.unpack_from("=I", args)[0]] = "wl_registry"
    elif kind == "wl_registry" and opcode == 0:
        length = struct.unpack_from("=I", args, 4)[0]
        name = args[8:8 + length - 1].decode()
        new_id = struct.unpack_from("=I", args, 8 + ((length + 3) & ~3) + 4)[0]
        objects[new_id] = name
    elif kind == "zwp_tablet_manager_v2" and opcode == 0:
        seat = struct.unpack_from("=I", args)[0]
    elif kind == "xdg_wm_base" and opcode == 2:
        new_id, surface = struct.unpack_from("=II", args)
        objects[new_id] = "xdg_surface"
        surfaces[new_id] = surface
    elif kind == "xdg_surface" and opcode == 1 and target is None:
        target = surfaces[obj]
    return True


listener = socket.socket(socket.AF_UNIX)
listener.bind(str(path))
listener.listen(8)
client, _ = listener.accept()
server = socket.socket(socket.AF_UNIX)
server.connect(str(runtime / display))
selector = selectors.DefaultSelector()
selector.register(client, selectors.EVENT_READ, server)
selector.register(server, selectors.EVENT_READ, client)
selector.register(listener, selectors.EVENT_READ, None)
pending = {client: bytearray(), server: bytearray()}
fds = {client: [], server: []}
index = 0
try:
    while True:
        for ready, _ in selector.select(0.005):
            source, destination = ready.fileobj, ready.data
            if source is listener:
                extra, _ = listener.accept()
                upstream = socket.socket(socket.AF_UNIX)
                upstream.connect(str(runtime / display))
                selector.register(extra, selectors.EVENT_READ, upstream)
                selector.register(upstream, selectors.EVENT_READ, extra)
                continue  # GPU drivers open separate discovery connections.
            if source.fileno() < 0:
                continue
            data, ancillary, _, _ = source.recvmsg(65536, socket.CMSG_SPACE(256))
            if not data:
                if source in (client, server):
                    sys.exit(0)
                for connection in (source, destination):
                    selector.unregister(connection)
                    connection.close()
                continue
            received = []
            for level, kind, raw in ancillary:
                if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
                    values = array.array("i")
                    values.frombytes(raw[:len(raw) - len(raw) % values.itemsize])
                    received.extend(values)
            if source in pending:
                buffer = pending[source]
                buffer.extend(data)
                fds[source].extend(received)
                data = bytearray()
                while len(buffer) >= 8:
                    size = struct.unpack_from("=I", buffer, 4)[0] >> 16
                    if len(buffer) < size:
                        break
                    message = bytes(buffer[:size])
                    del buffer[:size]
                    obj, header = struct.unpack_from("=II", message)
                    # The compositor does not own the injected tool. Let GTK
                    # use tool.set_cursor rather than asking cursor-shape to
                    # create a compositor object referring to that tool.
                    shape_global = source is server and objects.get(obj) == "wl_registry" \
                        and header & 0xFFFF == 0 and b"wp_cursor_shape_manager_v1\0" in message
                    if not shape_global and (source is not client or request(message)):
                        data.extend(message)
                received = fds[source] if data else []
                if data:
                    fds[source] = []
            if data:
                control = [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", received))] if received else []
                sent = destination.sendmsg([data], control)
                if sent < len(data):
                    destination.sendall(data[sent:])
            for fd in received:
                os.close(fd)
        if seat and not introduced:
            event(seat, 0, words(tablet))
            event(tablet, 0, string("CapyCanvas test tablet"))
            event(tablet, 1, words(0, 0))
            event(tablet, 3)
            event(seat, 1, words(tool))
            event(tool, 0, words(0x140))  # tablet-v2 pen
            event(tool, 1, words(0, 1))
            event(tool, 3, words(2))  # pressure
            event(tool, 4)
            introduced = True
        file = events / f"pen-{index}.json"
        if introduced and target and file.exists():
            contact = json.loads(file.read_text())
            serial += 1
            if not near:
                event(tool, 6, words(serial, tablet, target))
                near = True
            if "point" in contact:
                event(tool, 10, struct.pack("=ii", *(int(v * 256) for v in contact["point"])))
            if contact["pen"] == "down":
                event(tool, 11, words(32768))
                event(tool, 8, words(serial))
            elif contact["pen"] == "up":
                event(tool, 11, words(0))
                event(tool, 9)
            elif contact["pen"] == "button":
                event(tool, 17, words(serial, contact["button"], int(contact["down"])))
            elif contact["pen"] == "leave":
                event(tool, 7)
                near = False
            event(tool, 18, words(int(time.monotonic() * 1000) & 0xFFFFFFFF))
            index += 1
finally:
    path.unlink(missing_ok=True)
