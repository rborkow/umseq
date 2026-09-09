import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from collapse_matrix import collapse_rows, residency_result, order


class CollapseMatrixTests(unittest.TestCase):
    def test_failed_or_missing_intervention_is_not_accepted(self):
        lines = [f"STAR_INTEGRATE_COLLAPSE label={n} state=attempted attempted=1 result=0 errno=0 bytes=2097152 elapsed_ns=1" for n in ("Genome", "SA", "SAindex")]
        rows = collapse_rows("\n".join(lines), True)
        self.assertEqual(len(rows), 3)
        for text in ("\n".join(lines[:2]), "\n".join(lines).replace("result=0", "result=-1")):
            with self.assertRaises(ValueError):
                collapse_rows(text, True)
        with self.assertRaises(ValueError):
            collapse_rows("\n".join(lines), False)

    def test_on_requires_resident_coverage_off_only_records_it(self):
        maps = [{"VmFlags": "rd wr hg", "Rss": 4096, "Size": 4096, "AnonHugePages": 0, "extent_bytes": 4194304} for _ in range(3)]
        samples = [{"large_anonymous_mappings": maps, "timestamp_unix_s": 1}]
        calls = [{"label": n, "bytes": "2097152"} for n in ("Genome", "SA", "SAindex")]
        self.assertEqual(residency_result(samples, calls, False)["huge_bytes"], 0)
        with self.assertRaises(ValueError):
            residency_result(samples, calls, True)
        for m in maps:
            m["AnonHugePages"] = 4096
        self.assertGreater(residency_result(samples, calls, True)["huge_bytes"], 0)
        with self.assertRaises(ValueError):
            residency_result([], calls, True)

    def test_each_repeat_contains_all_four_arms(self):
        expected = {("bypass-off", False, False), ("gpu-off", True, False), ("bypass-on", False, True), ("gpu-on", True, True)}
        for repeat in range(1, 4):
            self.assertEqual(set(order(repeat)), expected)
        self.assertNotEqual(order(1), order(2))


if __name__ == "__main__":
    unittest.main()
