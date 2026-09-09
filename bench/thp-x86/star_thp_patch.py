#!/usr/bin/env python3
"""Extract the generator's THP hook and apply its stock-STAR port.

The generator is the source of truth for the hook body.  The stock port only
renames the generator's private build/runtime names so it can be built without
the rest of STAR_INTEGRATE.
"""
from __future__ import annotations

import argparse
import ast
from pathlib import Path


HERE = Path(__file__).resolve().parent
GENERATOR = HERE.parent / "star-integrate" / "make_star_integrate.py"
PACKED = "PackedArray.cpp"
GENOME = "Genome_genomeLoad.cpp"
GUARD = "#if defined(STAR_INTEGRATE) && defined(__linux__)\n"


def _string_constants(path: Path) -> list[str]:
    tree = ast.parse(path.read_text())
    return [node.value for node in ast.walk(tree) if isinstance(node, ast.Constant) and isinstance(node.value, str)]


def extract_generator_hook(generator: Path = GENERATOR) -> str:
    """Return the exact Linux hook block emitted by the generator."""
    candidates = []
    for value in _string_constants(generator):
        if "static void starIntegrateAdviseHuge" in value and GUARD in value:
            start = value.index(GUARD)
            end = value.index("#endif\n", start) + len("#endif\n")
            candidates.append(value[start:end])
    if not candidates:
        raise RuntimeError("generator does not contain one consistent THP hook")
    # Genome's emitted block also contains the unrelated cache-drop helper and
    # an extra fcntl include.  The shared advice hook itself must match.
    function = candidates[0][candidates[0].index("static void starIntegrateAdviseHuge") :]
    function = function[: function.index("\n#endif")]
    for candidate in candidates[1:]:
        other = candidate[candidate.index("static void starIntegrateAdviseHuge") :]
        other = other[: other.index("\nstatic void starIntegrateDropFile") if "\nstatic void starIntegrateDropFile" in other else other.index("\n#endif")]
        if other != function:
            raise RuntimeError("generator emits inconsistent THP hook bodies")
    return candidates[0][: candidates[0].index("static void starIntegrateAdviseHuge")] + function + "\n#endif\n"


def stock_hook_from_generator(generator: Path = GENERATOR) -> str:
    """Adapt only names that are private to the integrated build."""
    raw = extract_generator_hook(generator)
    # Order matters: the env var STAR_INTEGRATE_THP becomes STAR_THP (not STAR_THP_THP),
    # the build guard becomes STAR_THP_PATCH, and the perror prefix is renamed last.
    return (raw.replace("STAR_INTEGRATE_THP", "STAR_THP")
               .replace("#if defined(STAR_INTEGRATE) &&", "#if defined(STAR_THP_PATCH) &&")
               .replace("STAR_INTEGRATE", "STAR_THP"))


FROZEN_HOOK = HERE / "star_thp_hook.frozen.h"


def stock_hook(generator: Path = GENERATOR) -> str:
    """The frozen hook shipped with the bundle (self-contained on a Batch box).

    When the generator is present (in-repo), it must agree byte-for-byte with the frozen
    copy; test_patch_stock_star.py pins this so the bundle cannot drift from the source of
    truth silently.  On a box without the repo, the frozen copy is used alone.
    """
    frozen = FROZEN_HOOK.read_text()
    if generator.is_file():
        live = stock_hook_from_generator(generator)
        if live != frozen:
            raise RuntimeError("star_thp_hook.frozen.h no longer matches the generator; regenerate it")
    return frozen


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one anchor, found {count}")
    return text.replace(old, new, 1)


def patch_file(path: Path, generator: Path = GENERATOR) -> None:
    text = path.read_text()
    hook = stock_hook(generator)
    if path.name == PACKED:
        text = replace_once(text, '# include "PackedArray.h"\n',
                            '# include "PackedArray.h"\n' + hook, "PackedArray include")
        text = replace_once(text, "    charArray=new char[lengthByte];\n",
                            "    charArray=new char[lengthByte];\n"
                            "#if defined(STAR_THP_PATCH) && defined(__linux__)\n"
                            "    starIntegrateAdviseHuge(charArray,lengthByte);\n"
                            "#endif\n", "PackedArray allocation")
        path.write_text(text)
        return
    if path.name == GENOME:
        text = replace_once(text, '#include "genomeScanFastaFiles.h"\n',
                            '#include "genomeScanFastaFiles.h"\n' + hook, "Genome include")
        allocations = (
            ("                G1=new char[nGenomePass2+L+L];\n", ""),
            ("                    G1=new char[nGenome+L+L];\n", "                    SA.allocateArray();\n"),
            ("                    G1=new char[nGenome+L+L+genomeInsertL];\n", ""),
        )
        for i, anchor in enumerate(allocations):
            call = ("                starIntegrateAdviseHuge(G1,nGenomePass2+L+L);\n"
                    if i == 0 else
                    "                    starIntegrateAdviseHuge(G1,nGenome+L+L);\n"
                    if i == 1 else
                    "                    starIntegrateAdviseHuge(G1,nGenome+L+L+genomeInsertL);\n")
            allocation, suffix = anchor
            indent = allocation[:len(allocation) - len(allocation.lstrip())]
            insertion = allocation + "#if defined(STAR_THP_PATCH) && defined(__linux__)\n" + call + indent + "#endif\n" + suffix
            text = replace_once(text, allocation + suffix, insertion, f"Genome allocation {i + 1}")
        path.write_text(text)
        return
    raise RuntimeError(f"unsupported source file: {path}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="STAR 2.7.11b source tree")
    parser.add_argument("--generator", type=Path, default=GENERATOR)
    args = parser.parse_args()
    root = args.source
    for name in (GENOME, PACKED):
        patch_file(root / "source" / name if (root / "source" / name).exists() else root / name,
                   args.generator)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
