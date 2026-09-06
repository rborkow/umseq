# P2C seed lane coordination

**Actual tiny read-capture host1 completed exit0; shared lock verified FREE.** No remote workload currently active from this lane. The seed lane relinquishes its staged-validation reservation at the approved tiny replay milestone; coordinate a fresh slot before any further heavy host job.

- `~/uni-rnaseq-seed-lab/read-capture-host1.status`: capture_passed_requires_review,exit0. Originals `read-capture-run1`; local archive `experiments/star-seed/evidence/read-capture-run1/` in seed worktree.
- Actual stock/capture mapping both exit0 for12SE reads on each no-SJ/SJ index. Captured109/106 INNER calls. Orchestrator fresh sanitizer parser rerun validates counts and strict parity rerun passes; raw SAM alignment and SJ bytes identical. This is not independent search replay.
- Capture and tiny integrated replay APPROVED. Luna re-review closed all three blockers and reproduced109/106 actual four-field matches; durable integrated-replay-v2 evidence. No further worker or remote job launched at this milestone.
- No PE, full human-index or performance/GPU result; tiny SE SSIR-to-loader/search integration is independently approved. Installed STAR, prior evidence/variants, main BAM/QC workspace, services/settings untouched.

This lane has no reserved heavy-workload slot now. Obtain a fresh handoff and hold the shared lock for successor validation.
