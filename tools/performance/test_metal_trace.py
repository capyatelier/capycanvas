import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

from metal_trace import summarize


def value(tag, text=None, **attributes):
    node = ET.Element(tag, attributes)
    if text is not None:
        node.text = str(text)
    return node


def process(pid, **attributes):
    node = value("process", **attributes)
    node.append(value("pid", pid))
    return node


def gpu_row(start, duration, pid, encoder, buffer, channel="Vertex", state="Active", latency=0):
    return [value("start-time", start), value("sentinel") if duration is None else value("duration", duration),
            pid, value("gpu-state", state), value("gpu-channel-name", channel),
            value("metal-command-buffer-id", encoder), value("metal-command-buffer-id", buffer), value("duration", latency)]


def table(path, name, columns, rows):
    root = ET.Element("trace-query-result")
    schema = ET.SubElement(root, "schema", name=name)
    for column in columns:
        ET.SubElement(ET.SubElement(schema, "col"), "mnemonic").text = column
    for elements in rows:
        row = ET.SubElement(root, "row")
        row.extend(elements)
    ET.ElementTree(root).write(path)


class MetalTraceChecks(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.gpu = Path(self.directory.name) / "gpu.xml"
        self.encoders = Path(self.directory.name) / "encoders.xml"

    def run_report(self, rows, encoders):
        table(self.gpu, "metal-gpu-intervals", ["start", "duration", "process", "state", "channel-name", "encoder-id", "cmdbuffer-id", "start-latency"], rows)
        table(self.encoders, "metal-application-encoders-list", ["process", "encoder-id", "encoder-label"],
              [[process(42), value("metal-command-buffer-id", identity), value("metal-object-label", label)] for identity, label in encoders])
        return summarize(self.gpu, self.encoders, 42)

    def test_process_filter_references_and_overlapping_gpu_stages(self):
        rows = [gpu_row(0, 10_000, process(42, id="p"), 1, 10, latency=900_000),
                gpu_row(5_000, 15_000, value("process", ref="p"), 1, 10, "Fragment"),
                gpu_row(30_000, 10_000, value("process", ref="p"), 2, 20, "Compute"),
                gpu_row(0, 1_000_000, process(99), 3, 30),
                gpu_row(20_000, 10_000, value("process", ref="p"), 2, 20, state="Blocked")]
        report = self.run_report(rows, [(1, "clear"), (2, "paint")])
        self.assertEqual(report["target_gpu_rows"], 4)
        self.assertEqual(report["other_process_rows"], 1)
        self.assertEqual(report["states"], {"Active": 3, "Blocked": 1})
        self.assertAlmostEqual(report["gpu"]["active_union_ms"], .03)
        self.assertAlmostEqual(report["gpu"]["raw_stage_sum_ms"], .035)
        self.assertAlmostEqual(report["per_command_buffer_active_union_ms"]["max"], .02)
        self.assertAlmostEqual(report["cpu_to_gpu_start_latency_ms"]["max"], .9)
        self.assertEqual(report["gpu_intervals_without_cpu_encoder"], 0)
        self.assertEqual(report["cpu_encoders_without_gpu_intervals"], 0)

    def test_partial_window_and_invalid_durations_remain_visible(self):
        report = self.run_report([gpu_row(0, 10, process(42), 1, 10),
                                  gpu_row(20, None, process(42), 2, 20),
                                  gpu_row(30, 0, process(42), 3, 30),
                                  gpu_row(40, -1, process(42), 4, 40)], [(2, "missing")])
        self.assertEqual(report["invalid_active_intervals"], 3)
        self.assertEqual(report["gpu"]["intervals"], 1)
        self.assertEqual(report["gpu_intervals_without_cpu_encoder"], 1)
        self.assertEqual(report["cpu_encoders_without_gpu_intervals"], 1)
        self.assertTrue(any("coverage differ" in w for w in report["warnings"]))

    def test_duplicate_encoder_identity_does_not_invent_a_label_match(self):
        report = self.run_report([gpu_row(0, 10, process(42), 1, 10)], [(1, "first"), (1, "second")])
        self.assertEqual(report["duplicate_cpu_encoder_rows"], 1)
        self.assertEqual(report["encoder_labels"], {})
        self.assertEqual(report["gpu_intervals_without_cpu_encoder"], 1)

    def test_unresolved_reference_and_missing_target_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "reference"):
            self.run_report([gpu_row(0, 10, value("process", ref="missing"), 1, 10)], [])
        with self.assertRaisesRegex(ValueError, "requested process"):
            self.run_report([gpu_row(0, 10, process(99), 1, 10)], [])

    def test_empty_active_measurement_is_not_a_zero_duration_result(self):
        report = self.run_report([gpu_row(0, 10, process(42), 1, 10, state="Blocked")], [])
        self.assertIsNone(report["gpu"]["stage_duration_ms"]["p50"])
        self.assertTrue(any("No positive Active" in w for w in report["warnings"]))


if __name__ == "__main__":
    unittest.main()
