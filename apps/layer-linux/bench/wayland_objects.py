import struct
from pathlib import Path
from xml.etree import ElementTree

SERVER_BASE = 0xFF000000


class Objects:
    def __init__(self, reserved):
        self.reserved = reserved
        self.kinds = {1: "wl_display"}
        self.interfaces = {}
        paths = [Path("/usr/share/wayland/wayland.xml"), *Path("/usr/share/wayland-protocols").rglob("*.xml")]
        for path in paths:
            for interface in ElementTree.parse(path).getroot().findall("interface"):
                name = interface.get("name")
                version = int(interface.get("version"))
                if name in self.interfaces and self.interfaces[name][0] >= version:
                    continue
                messages = {direction: [[(arg.get("type"), arg.get("interface")) for arg in message.findall("arg")]
                                       for message in interface.findall(direction)] for direction in ("request", "event")}
                self.interfaces[name] = (version, messages)

    def client_id(self, value):
        return value + self.reserved if value >= SERVER_BASE else value

    def server_id(self, value):
        return value - self.reserved if value >= SERVER_BASE else value

    def translate(self, message, incoming):
        result = bytearray(message)
        obj, header = struct.unpack_from("=II", result)
        visible = self.client_id(obj) if incoming else obj
        kind = self.kinds.get(visible)
        mapping = self.client_id if incoming else self.server_id
        struct.pack_into("=I", result, 0, mapping(obj))
        definition = self.interfaces.get(kind)
        if definition is None:
            return bytes(result)
        arguments = definition[1]["event" if incoming else "request"][header & 0xFFFF]
        offset = 8
        for arg_type, interface in arguments:
            if arg_type == "fd":
                continue
            if arg_type in ("string", "array"):
                size = struct.unpack_from("=I", result, offset)[0]
                offset += 4 + ((size + 3) & ~3)
                continue
            if arg_type == "new_id" and interface is None:
                size = struct.unpack_from("=I", result, offset)[0]
                interface = result[offset + 4:offset + 3 + size].decode()
                offset += 8 + ((size + 3) & ~3)
            value = struct.unpack_from("=I", result, offset)[0]
            if arg_type in ("object", "new_id"):
                struct.pack_into("=I", result, offset, mapping(value))
                if arg_type == "new_id":
                    self.kinds[self.client_id(value) if incoming else value] = interface
            offset += 4
        return bytes(result)
