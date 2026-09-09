#!/usr/bin/env python3
"""Build and run the pinned STAR producer prefix test with STAR's C++11 ABI."""

import shutil
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = Path("/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source")
CLANG = Path("/opt/homebrew/opt/llvm/bin/clang++")


def main():
    if not SOURCE.exists():
        raise SystemExit("pinned STAR source unavailable: " + str(SOURCE))
    compiler = str(CLANG if CLANG.exists() else shutil.which("clang++"))
    if compiler == "None":
        raise SystemExit("clang++ unavailable")
    with tempfile.TemporaryDirectory(prefix="star-window-prefix-") as tmp:
        root = Path(tmp)
        # STAR normally generates this include in its build directory.  Keep the
        # pinned source read-only by compiling a temporary Parameters.cpp copy.
        parameters_cpp = root / "Parameters.cpp"
        shutil.copy2(SOURCE / "Parameters.cpp", parameters_cpp)
        with (root / "parametersDefault.xxd").open("wb") as generated:
            subprocess.run(
                [
                    "xxd",
                    "-i",
                    "-n",
                    "parametersDefault",
                    str(SOURCE / "parametersDefault"),
                ],
                check=True,
                stdout=generated,
            )
        exe = root / "test_window_prefix"
        command = [
            compiler,
            "-std=c++11",
            "-fopenmp",
            '-DCOMPILATION_TIME_PLACE="test"',
            '-DGIT_BRANCH_COMMIT_DIFF="test"',
            "-I" + str(SOURCE),
            "-I" + str(HERE),
            str(HERE / "test_window_prefix.cpp"),
            str(parameters_cpp),
            str(SOURCE / "Genome.cpp"),
            str(SOURCE / "PackedArray.cpp"),
            str(SOURCE / "InOutStreams.cpp"),
            str(SOURCE / "SequenceFuns.cpp"),
            "-Wl,-dead_strip",
            "-o",
            str(exe),
        ]
        subprocess.run(command, check=True)
        subprocess.run([str(exe)], check=True)


if __name__ == "__main__":
    main()
