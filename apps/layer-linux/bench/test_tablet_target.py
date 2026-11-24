import struct
import unittest
from tablet_target import TabletTarget, pressure


def words(*values):
    return struct.pack("=" + "I" * len(values), *values)


class TabletTargetTests(unittest.TestCase):
    def test_contacts_carry_their_pressure_and_default_to_half(self):
        self.assertEqual([pressure(contact) for contact in ({"pen": "down"}, {"pen": "move", "pressure": .25},
                                                            {"pen": "move", "pressure": 1.5}, {"pen": "down", "pressure": -1})],
                         [32768, 16384, 65535, 0])

    def test_new_windows_receive_the_pen_after_activation_without_stealing_it_while_inactive(self):
        target = TabletTarget()
        for surface, xdg, toplevel in [(20, 21, 22), (30, 31, 32), (40, 41, 42)]:
            previous = target.current
            target.request("xdg_wm_base", 5, 2, words(xdg, surface))
            target.request("xdg_surface", xdg, 1, words(toplevel))
            target.event("xdg_toplevel", toplevel, 0, words(1600, 1000, 4, 1))
            self.assertEqual(target.current, previous)
            target.event("xdg_toplevel", toplevel, 0, words(1600, 1000, 8, 1, 4))
            self.assertEqual(target.current, surface)
        target.event("xdg_toplevel", 22, 0, words(1600, 1000, 4, 1))
        target.request("xdg_toplevel", 22, 0, b"")
        target.request("xdg_surface", 21, 0, b"")
        target.request("wl_surface", 20, 0, b"")
        self.assertEqual(target.current, 40)
        target.event("xdg_toplevel", 32, 0, words(1600, 1000, 4, 4))
        self.assertEqual(target.current, 30)
        target.event("xdg_toplevel", 32, 0, words(1600, 1000, 0))
        self.assertIsNone(target.current)

    def test_destroying_and_reusing_protocol_ids_never_leaves_a_stale_pen_target(self):
        for kind, destroyed in [("xdg_toplevel", 22), ("xdg_surface", 21), ("wl_surface", 20)]:
            target = TabletTarget()
            target.request("xdg_wm_base", 5, 2, words(21, 20))
            target.request("xdg_surface", 21, 1, words(22))
            target.event("xdg_toplevel", 22, 0, words(1600, 1000, 4, 4))
            target.request(kind, destroyed, 0, b"")
            self.assertIsNone(target.current)
            target.event("xdg_toplevel", 22, 0, words(1600, 1000, 4, 4))
            self.assertIsNone(target.current)
            target.request("xdg_wm_base", 5, 2, words(21, 50))
            target.request("xdg_surface", 21, 1, words(22))
            target.event("xdg_toplevel", 22, 0, words(1600, 1000, 4, 4))
            self.assertEqual(target.current, 50)


if __name__ == "__main__":
    unittest.main()
