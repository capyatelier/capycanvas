import struct


class TabletTarget:
    def __init__(self):
        self.surfaces = {}
        self.toplevels = {}
        self.current = None

    def request(self, kind, obj, opcode, args):
        if kind == "xdg_wm_base" and opcode == 2:
            xdg, surface = struct.unpack_from("=II", args)
            self.surfaces[xdg] = surface
        elif kind == "xdg_surface" and opcode == 1:
            self.toplevels[struct.unpack_from("=I", args)[0]] = self.surfaces[obj]
        elif opcode == 0:
            if kind == "xdg_toplevel":
                surface = self.toplevels.pop(obj, None)
            elif kind == "xdg_surface":
                surface = self.surfaces.pop(obj, None)
            elif kind == "wl_surface":
                surface = obj
            else:
                return
            if kind in ("xdg_surface", "wl_surface"):
                self.surfaces = {key: value for key, value in self.surfaces.items() if value != surface}
                self.toplevels = {key: value for key, value in self.toplevels.items() if value != surface}
            if surface == self.current:
                self.current = None

    def event(self, kind, obj, opcode, args):
        if kind == "xdg_toplevel" and opcode == 0 and obj in self.toplevels:
            size = struct.unpack_from("=I", args, 8)[0]
            states = struct.unpack_from("=" + "I" * (size // 4), args, 12)
            if 4 in states:
                self.current = self.toplevels[obj]
            elif self.current == self.toplevels[obj]:
                self.current = None


def pressure(contact):
    return round(min(1, max(0, contact.get("pressure", .5))) * 65535)
