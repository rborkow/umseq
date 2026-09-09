import subprocess, sys, tempfile, unittest
from pathlib import Path
HERE=Path(__file__).resolve().parent
HEADER="repeat\tarm\twall_s\tuser_s\tsys_s\tmax_rss_kib\texit\tanonhuge_pages_kib\tcmp_sam\tcmp_sj\n"
class AnalyzeTest(unittest.TestCase):
    def test_synthetic(self):
        rows=[("1","stock","10","8","2","1","0","0","0","0"),("1","patched-off","11","9","2","1","0","0","0","0"),("1","patched-on","9","7","2","1","0","100","0","0")]
        with tempfile.TemporaryDirectory() as d:
            f=Path(d)/"x.tsv"; f.write_text(HEADER+"\n".join("\t".join(r) for r in rows)+"\n")
            out=subprocess.run([sys.executable,str(HERE/"analyze_thp_matrix.py"),str(f)],text=True,capture_output=True)
            self.assertEqual(out.returncode,0); self.assertIn("vs stock",out.stdout); self.assertIn("madvise alone, same binary",out.stdout)
    def test_refuses_ratio_on_cmp_difference(self):
        with tempfile.TemporaryDirectory() as d:
            f=Path(d)/"x.tsv"; f.write_text(HEADER+"1\tstock\t1\t1\t0\t1\t0\t0\t1\t0\n")
            out=subprocess.run([sys.executable,str(HERE/"analyze_thp_matrix.py"),str(f)],text=True,capture_output=True)
            self.assertNotIn("vs stock",out.stdout); self.assertEqual(out.returncode,1)
if __name__ == "__main__": unittest.main()

