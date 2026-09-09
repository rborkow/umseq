import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from observe_index_residency import large_anonymous_mappings


class ResidencyParserTests(unittest.TestCase):
    def test_preserves_large_mapping_fields_without_global_attribution(self):
        text = (
            "1000-5000 rw-p 00000000 00:00 0 \n"
            "Rss: 12 kB\nAnonHugePages: 8 kB\nTHPeligible: 1\nVmFlags: rd wr hg\n"
            "6000-a000 r--p 00000000 08:01 2 /file\nAnonHugePages: 0 kB\n"
            "b000-c000 rw-p 00000000 00:00 0 \nRss: 4 kB\n"
        )
        rows = large_anonymous_mappings(text, minimum_bytes=8192)
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["range"], "1000-5000")
        self.assertEqual(rows[0]["AnonHugePages"], 8)
        self.assertEqual(rows[0]["THPeligible"], 1)
        self.assertEqual(rows[0]["VmFlags"], "rd wr hg")

    def test_missing_huge_page_field_is_not_measured_zero(self):
        rows = large_anonymous_mappings("1000-5000 rw-p 0 00:00 0\nRss: 12 kB\n", 8192)
        self.assertNotIn("AnonHugePages", rows[0])


if __name__ == "__main__":
    unittest.main()
