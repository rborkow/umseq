# T3 Mac validation

Final fmt, clippy, workspace tests, C11 ABI and clang-format checks passed. `checks.tsv` records command outcomes. Raw final compiler/test output is retained locally as `clippy.log` and `workspace-tests.log`; no benchmark timing or GPU result was measured. The helper's full STAR include check was blocked by unavailable `omp.h`. The final workspace run includes the independent prefix-grid test and the malformed-batch alias regression. See `bench/PHASE2C-integrate-v2-t3.md` for the remaining Spark gates.
