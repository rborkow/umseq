#!/usr/bin/env python3
"""Run real producer/coordinator and accepted generated read-loading prefix.

The two mapChunk hook statements and oneRead through readFileType are exported
from the accepted generator. Mapping/output are outside this stream contract.
--source-dir permits executable RED on a saved implementation, without edits.
"""

import argparse
import importlib.util
import shutil
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
STAR = Path("/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source")
REPLAY = Path("/private/tmp/p2c-capture-local-01/helpers")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-dir", type=Path, default=HERE)
    parser.add_argument("--case", action="append")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="star-window-contract-") as directory:
        root = Path(directory)
        generator_path = root / "accepted_generator.py"
        generator_path.write_bytes(
            subprocess.check_output(
                ["git", "show", "89f005c:bench/star-integrate/make_star_integrate.py"],
                cwd=HERE,
            )
        )
        spec = importlib.util.spec_from_file_location(
            "accepted_generator", generator_path
        )
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)

        class ReplaceOnce:
            def replace_once(self, text, old, new):
                assert text.count(old) == 1
                return text.replace(old, new, 1)

        def generated(name):
            return generator.patch(ReplaceOnce(), name, (STAR / name).read_text())

        one = generated("ReadAlign_oneRead.cpp")
        one = one.split("    statsRA.readN++;", 1)[0] + "    return mapOneRead();\n}\n"
        (root / "generated_oneRead.inc").write_text(one)
        mapping = generated("ReadAlign_mapOneRead.cpp")
        entry = mapping.split("int ReadAlign::mapOneRead() {", 1)[1].split(
            "    #ifdef OFF_BEFORE_SEEDING", 1
        )[0]
        assert "star_integrate::begin_map(*this);" in entry
        (root / "generated_map_entry.inc").write_text(entry)
        chunk = generated("ReadAlignChunk_mapChunk.cpp")
        start = chunk.index("        star_integrate::prepare_window(*this);")
        stop = chunk.index("//map one read", start) + len("//map one read")
        (root / "generated_chunk.inc").write_text(
            "void ReadAlignChunk::mapChunk() {\nint readStatus;\n"
            + chunk[start:stop].replace(
                "\n        readStatus=",
                "\n        inspect_peek(*this);\n        readStatus=",
            )
            + "\nlast_status=readStatus;\n}\n"
        )
        for name in (
            "star_integrate.cpp",
            "star_integrate.hpp",
            "star_integrate_window.cpp",
        ):
            shutil.copy2(args.source_dir / name, root / name)
        shutil.copy2(HERE / "test_window_contract.cpp", root)
        shutil.copy2(STAR / "Parameters.cpp", root)
        with (root / "parametersDefault.xxd").open("wb") as output:
            subprocess.run(
                [
                    "xxd",
                    "-i",
                    "-n",
                    "parametersDefault",
                    str(STAR / "parametersDefault"),
                ],
                stdout=output,
                check=True,
            )
        compiler = "/opt/homebrew/opt/llvm/bin/clang++"
        exe = root / "contract"
        units = [
            "Genome.cpp",
            "PackedArray.cpp",
            "InOutStreams.cpp",
            "SequenceFuns.cpp",
            "readLoad.cpp",
            "ClipMate_clip.cpp",
            "ClipCR4.cpp",
            "Stats.cpp",
            "Transcript.cpp",
        ]
        command = [
            compiler,
            "-std=c++11",
            "-fopenmp",
            "-pthread",
            '-DCOMPILATION_TIME_PLACE="test"',
            '-DGIT_BRANCH_COMMIT_DIFF="test"',
            "-I" + str(root),
            "-I" + str(HERE),
            "-I" + str(STAR),
            "-I" + str(REPLAY),
            str(root / "test_window_contract.cpp"),
            str(root / "Parameters.cpp"),
        ]
        command += [str(STAR / name) for name in units]
        command += [str(REPLAY / "sha256.cpp"), "-Wl,-dead_strip", "-o", str(exe)]
        subprocess.run(command, check=True, timeout=90)
        cases = args.case or [
            "budget",
            "tail",
            "empty-successor",
            "gap",
            "overshoot",
            "refusal",
            "eof0",
            "eof1",
            "fail0",
            "fail1",
            "mismatch0",
            "mismatch1",
            "rollback",
        ]
        for case in cases:
            result = subprocess.run(
                [str(exe), case], capture_output=True, text=True, timeout=45
            )
            if case == "rollback":
                assert (
                    result.returncode == -6
                    and "input lookahead rewind failed" in result.stderr
                ), result
            elif case.startswith("mismatch"):
                assert (
                    result.returncode == 42
                    and "read files are not consistent" in result.stderr
                ), result
            else:
                assert result.returncode == 0, (case, result.stdout, result.stderr)
            print(result.stdout, end="")
            print(case + ": PASS", flush=True)


if __name__ == "__main__":
    main()
