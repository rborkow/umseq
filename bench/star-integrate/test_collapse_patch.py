#!/usr/bin/env python3
"""Focused RED/GREEN checks for generated opt-in index collapse hooks."""

import importlib.util
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
STAR = Path(os.environ.get("STAR_SOURCE_DIR", "/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source"))
CXX = os.environ.get("CXX") or shutil.which("clang++") or shutil.which("c++")

spec = importlib.util.spec_from_file_location(
    "star_integrate_generator", HERE / "make_star_integrate.py"
)
generator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(generator)


class ReplaceOnce:
    def replace_once(self, text, old, new):
        if text.count(old) != 1:
            raise ValueError("missing or duplicate pinned hook")
        return text.replace(old, new, 1)


def generated(name):
    return generator.patch(ReplaceOnce(), name, (STAR / name).read_text())


def collapse_helper(source):
    start = source.index("static bool starIntegrateCollapseRequested()")
    stop = source.index("static void starIntegrateDropFile", start)
    return source[start:stop]


class CollapsePatch(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not STAR.exists():
            raise unittest.SkipTest("pinned private source unavailable")
        cls.genome = generated("Genome_genomeLoad.cpp")
        cls.star = generated("STAR.cpp")

    def test_generated_order_and_real_array_extents(self):
        genome = self.genome
        self.assertIn("#ifndef MADV_COLLAPSE\n#define MADV_COLLAPSE 25", genome)
        self.assertIn("defined(__linux__)", genome)
        self.assertLess(genome.index("fstreamReadBig(GenomeIn,G,nGenome)"), genome.index("SAiIn.close();"))
        self.assertLess(genome.index("fstreamReadBig(SAin,SA.charArray, SA.lengthByte)"), genome.index("SAiIn.close();"))
        self.assertLess(genome.index("fstreamReadBig(SAiIn,SAi.charArray, SAi.lengthByte)"), genome.index("SAiIn.close();"))
        close = genome.index("SAiIn.close();")
        drop = genome.index('starIntegrateDropFile(pGe.gDir+"/Genome");', close)
        collapse = genome.index('starIntegrateCollapseIndex("Genome",G1,starIntegrateG1Extent);', close)
        self.assertLess(close, drop)
        self.assertLess(drop, collapse)
        self.assertLess(
            self.star.index("genomeMain.genomeLoad();"),
            self.star.index("star_integrate::setup(P, genomeMain);"),
        )
        self.assertIn('starIntegrateCollapseIndex("SA",SA.charArray,SA.lengthByte);', genome)
        self.assertIn('starIntegrateCollapseIndex("SAindex",SAi.charArray,SAi.lengthByte);', genome)
        self.assertIn('if (pGe.gLoad=="NoSharedMemory")', genome)

    def test_generated_helper_runtime_mock(self):
        helper = collapse_helper(self.genome)
        harness = """\
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#ifndef MADV_COLLAPSE
#define MADV_COLLAPSE 25
#endif
static int calls, failure;
static void *last_address;
static size_t last_bytes;
int mockMadvise(void *address, size_t bytes, int advice) {
  ++calls;
  last_address = address;
  last_bytes = bytes;
  assert(advice == MADV_COLLAPSE);
  if (failure) { errno = EIO; return -1; }
  return 0;
}
#define madvise mockMadvise
""" + helper + """
static void reset(const char *collapse, const char *thp, int fail) {
  calls = 0; failure = fail; last_address = 0; last_bytes = 0;
  if (collapse) setenv("STAR_INTEGRATE_COLLAPSE_INDEX", collapse, 1);
  else unsetenv("STAR_INTEGRATE_COLLAPSE_INDEX");
  if (thp) setenv("STAR_INTEGRATE_THP", thp, 1);
  else unsetenv("STAR_INTEGRATE_THP");
}
static void expect_line(const char *needle) {
  fflush(stderr); rewind(stderr);
  char line[512] = {0};
  fread(line, 1, sizeof(line) - 1, stderr);
  assert(strstr(line, needle) != 0);
  freopen("/dev/null", "w+", stderr);
}
int main() {
  freopen("/tmp/star-collapse-test.log", "w+", stderr);
  reset(0, 0, 0);
  starIntegrateCollapseIndex("Genome", (char *)0x100123, 0x500000);
  assert(calls == 0); expect_line("state=skipped-policy attempted=0 result=0 errno=0 bytes=0");
  freopen("/tmp/star-collapse-test.log", "w+", stderr);
  reset("1", 0, 0);
  starIntegrateCollapseIndex("SA", (char *)0x100123, 0x500000);
  assert(calls == 1 && last_address == (void *)0x200000 && last_bytes == 0x400000);
  expect_line("label=SA state=attempted attempted=1 result=0 errno=0 bytes=4194304");
  freopen("/tmp/star-collapse-test.log", "w+", stderr);
  reset("1", "0", 0);
  starIntegrateCollapseIndex("SAindex", (char *)0x100123, 0x500000);
  assert(calls == 0); expect_line("state=skipped-policy attempted=0 result=0 errno=0 bytes=0");
  freopen("/tmp/star-collapse-test.log", "w+", stderr);
  reset("1", 0, 0);
  starIntegrateCollapseIndex("small", (char *)0x100123, 0xffff);
  assert(calls == 0); expect_line("state=skipped-span attempted=0 result=0 errno=0 bytes=0");
  freopen("/tmp/star-collapse-test.log", "w+", stderr);
  reset("1", 0, 0);
  starIntegrateCollapseIndex("empty", (char *)0x100123, 0);
  assert(calls == 0); expect_line("state=skipped-span attempted=0 result=0 errno=0 bytes=0");
  freopen("/tmp/star-collapse-test.log", "w+", stderr);
  reset("1", 0, 1);
  starIntegrateCollapseIndex("failed", (char *)0x100123, 0x500000);
  assert(calls == 1); expect_line("state=failed attempted=1 result=-1 errno=5 bytes=4194304");
  const char *offValues[] = {"0", "10", "", "true"};
  for (const char *value : offValues) {
    freopen("/tmp/star-collapse-test.log", "w+", stderr);
    reset(value, 0, 0);
    starIntegrateCollapseIndex("off", (char *)0x100123, 0x500000);
    assert(calls == 0);
    expect_line("state=skipped-policy attempted=0");
  }
  reset("1", "1", 0);
  starIntegrateCollapseIndex("null", 0, 0x500000);
  starIntegrateCollapseIndex("overflow-end", (char *)(UINTPTR_MAX - 0x100), 0x500000);
  starIntegrateCollapseIndex("overflow-align", (char *)(UINTPTR_MAX - 0x100), 0x80);
  assert(calls == 0);
  starIntegrateCollapseIndex("exact", (char *)0x200000, 0x200000);
  assert(calls == 1 && last_address == (void *)0x200000 && last_bytes == 0x200000);
  return 0;
}
"""
        with tempfile.TemporaryDirectory(prefix="star-collapse-patch-") as directory:
            source = Path(directory) / "collapse_mock.cpp"
            executable = Path(directory) / "collapse_mock"
            source.write_text(harness.replace("/tmp/star-collapse-test.log", str(Path(directory) / "stderr.log")))
            subprocess.run(
                [CXX, "-std=c++11", str(source), "-o", str(executable)],
                check=True,
                timeout=30,
            )
            subprocess.run([str(executable)], check=True, timeout=30)

    def test_mac_guard_compiles_without_linux_definitions(self):
        with tempfile.TemporaryDirectory(prefix="star-collapse-mac-") as directory:
            source = Path(directory) / "Genome_genomeLoad.cpp"
            object_file = Path(directory) / "Genome_genomeLoad.o"
            source.write_text(self.genome)
            subprocess.run(
                [
                    CXX,
                    "-std=c++11",
                    "-U__linux__",
                    "-DSTAR_INTEGRATE=1",
                    "-I" + str(STAR),
                    "-c",
                    str(source),
                    "-o",
                    str(object_file),
                ],
                check=True,
                timeout=30,
            )


if __name__ == "__main__":
    unittest.main()
