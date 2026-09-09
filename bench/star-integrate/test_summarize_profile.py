import tempfile
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from summarize_profile import parse_stacks


class ProfileSummaryTests(unittest.TestCase):
    def test_leaf_dso_and_first_user_caller_not_name_heuristics(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "stacks.txt"
            path.write_text(
                " 12 \n\tffff fault ([kernel.kallsyms])\n"
                "\t100 memcpy (/usr/lib/libc.so)\n"
                "\t200 producer (/accepted/STAR)\n\n"
                " 13 \n\t300 kernel_named_user (/accepted/STAR)\n"
            )
            result = parse_stacks(path)
            self.assertEqual(result["total"], 2)
            self.assertEqual(result["kernel_samples"], 1)
            self.assertEqual(result["kernel_caller"][("libc.so", "memcpy")], 1)
            self.assertEqual(result["kernel_application"][("STAR", "producer")], 1)
            self.assertEqual(result["leaf"][("STAR", "kernel_named_user")], 1)

    def test_unrecognized_or_frameless_samples_fail_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.txt"
            for text in (" 12\nBAD\n", " 12\n\n", "\t123 foo (/lib.so)\n"):
                path.write_text(text)
                with self.assertRaises(ValueError):
                    parse_stacks(path)


if __name__ == "__main__":
    unittest.main()
