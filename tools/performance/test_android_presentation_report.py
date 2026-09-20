import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("report", Path(__file__).with_name("android-presentation-report.py"))
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)


class PresentationReportTest(unittest.TestCase):
    def rows(self, latches=True):
        rows = [{"kind": "bounds", "ts": "0", "dur": "10000000000", "name": ""}]
        for i in range(600):
            rows.append({"kind": "present", "ts": str(i * 16666666), "dur": "500000", "name": ""})
            if latches:
                rows.append({"kind": "latch", "ts": str(i * 16666666), "dur": "1000", "name": ""})
        return rows

    def test_fast_present_without_latches_is_not_progress(self):
        result = report.summarize(self.rows(False))
        self.assertEqual(result["present_call_ms"]["max"], .5)
        self.assertGreater(result["latch_gap_ms"]["max"], 9900)
        self.assertFalse(report.check(result))

    def test_continuous_latches_pass(self):
        self.assertTrue(report.check(report.summarize(self.rows())))

    def test_trailing_freeze_is_not_hidden(self):
        rows = [r for r in self.rows() if r["kind"] != "latch" or int(r["ts"]) < 5000000000]
        self.assertFalse(report.check(report.summarize(rows)))

    def test_trace_loss_rejects_qualification(self):
        rows = self.rows() + [{"kind": "error", "ts": "0", "dur": "1", "name": "ftrace_cpu_overrun"}]
        self.assertFalse(report.check(report.summarize(rows)))

    def test_zoom_units(self):
        rows = self.rows() + [{"kind": "zoom", "ts": "0", "dur": str(z), "name": ""} for z in [2000, 1600000]]
        self.assertEqual(report.summarize(rows)["zoom_percent"], {"min": 2, "max": 1600})


if __name__ == "__main__":
    unittest.main()
