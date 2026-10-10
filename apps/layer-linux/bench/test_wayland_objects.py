import struct
import unittest
from wayland_objects import Objects, SERVER_BASE


def words(*values):
    return struct.pack("=" + "I" * len(values), *values)


def string(value):
    value = value.encode() + b"\0"
    return words(len(value)) + value + bytes((-len(value)) % 4)


def message(obj, opcode, payload=b""):
    return words(obj, ((len(payload) + 8) << 16) | opcode) + payload


class ObjectTranslationTests(unittest.TestCase):
    def setUp(self):
        self.objects = Objects(2)

    def test_primary_selection_offer_does_not_replace_injected_tablet(self):
        self.objects.kinds[35] = "zwp_primary_selection_device_v1"
        offer = message(35, 0, words(SERVER_BASE))
        self.assertEqual(self.objects.translate(offer, True), message(35, 0, words(SERVER_BASE + 2)))
        self.assertEqual(self.objects.kinds[SERVER_BASE + 2], "zwp_primary_selection_offer_v1")
        self.assertEqual(self.objects.translate(message(SERVER_BASE, 0, string("text/plain")), True),
                         message(SERVER_BASE + 2, 0, string("text/plain")))
        self.assertEqual(self.objects.translate(message(35, 1, words(SERVER_BASE)), True),
                         message(35, 1, words(SERVER_BASE + 2)))
        request = message(SERVER_BASE + 2, 0, string("text/plain"))
        self.assertEqual(self.objects.translate(request, False), message(SERVER_BASE, 0, string("text/plain")))

    def test_binding_and_core_data_offers_preserve_non_object_arguments(self):
        self.objects.translate(message(1, 1, words(2)), False)
        bind = message(2, 0, words(9) + string("wl_data_device_manager") + words(3, 12))
        self.assertEqual(self.objects.translate(bind, False), bind)
        self.assertEqual(self.objects.kinds[12], "wl_data_device_manager")
        self.objects.translate(message(12, 1, words(35, 7)), False)
        self.objects.translate(message(35, 0, words(SERVER_BASE)), True)
        entered = message(35, 1, words(SERVER_BASE, 50, SERVER_BASE, SERVER_BASE, SERVER_BASE))
        expected = message(35, 1, words(SERVER_BASE, 50, SERVER_BASE, SERVER_BASE, SERVER_BASE + 2))
        self.assertEqual(self.objects.translate(entered, True), expected)
        self.objects.kinds[50] = "wl_surface"
        attach = message(50, 1, words(SERVER_BASE + 2, SERVER_BASE, SERVER_BASE))
        self.assertEqual(self.objects.translate(attach, False), message(50, 1, words(SERVER_BASE, SERVER_BASE, SERVER_BASE)))


if __name__ == "__main__":
    unittest.main()
