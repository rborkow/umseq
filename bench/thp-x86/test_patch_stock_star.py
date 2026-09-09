import importlib.util
from pathlib import Path
import unittest


HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("star_thp_patch", HERE / "star_thp_patch.py")
assert spec and spec.loader
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


class PatchTest(unittest.TestCase):
    def test_raw_hook_is_exactly_generator_emission(self):
        raw = mod.extract_generator_hook()
        self.assertEqual(raw, mod.extract_generator_hook(mod.GENERATOR))
        self.assertIn("STAR_INTEGRATE_THP", raw)
        self.assertIn("sysconf(_SC_PAGESIZE)", raw)
        self.assertIn("perror(\"STAR_INTEGRATE madvise(MADV_HUGEPAGE)\")", raw)

    def test_stock_adaptation_is_narrow(self):
        adapted = mod.stock_hook()
        self.assertIn("STAR_THP", adapted)
        self.assertIn("STAR_THP_PATCH", adapted)
        self.assertNotIn("STAR_INTEGRATE_THP", adapted)

    def test_patch_requires_all_stock_anchors(self):
        from tempfile import TemporaryDirectory
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "PackedArray.cpp").write_text('# include "PackedArray.h"\n    charArray=new char[lengthByte];\n')
            (root / "Genome_genomeLoad.cpp").write_text(
                '#include "genomeScanFastaFiles.h"\n'
                '                G1=new char[nGenomePass2+L+L];\n'
                '                    G1=new char[nGenome+L+L];\n                    SA.allocateArray();\n'
                '                    G1=new char[nGenome+L+L+genomeInsertL];\n')
            mod.patch_file(root / "PackedArray.cpp")
            mod.patch_file(root / "Genome_genomeLoad.cpp")
            result = (root / "Genome_genomeLoad.cpp").read_text()
            self.assertEqual(result.count("starIntegrateAdviseHuge(G1"), 3)
            self.assertEqual((root / "PackedArray.cpp").read_text().count("starIntegrateAdviseHuge"), 2)


if __name__ == "__main__":
    unittest.main()
