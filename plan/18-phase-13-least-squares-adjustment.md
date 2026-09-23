<!-- Katana plan, section 18 of 47. Index: ../PLAN.MD. Previous: 17-phase-12-survey-calculations.md. Next: 19-phase-14-terrain-engine.md -->

# 18. Phase 13 — Least-Squares Adjustment

**STATUS: DELIVERED.** `least_squares.cpp`, `lsq_core.hpp`,
`network_adjustment.cpp`, `error_propagation.cpp`, `statistics.cpp`.

A genuine weighted least-squares solve with residuals, variance factor and
error ellipses - Eigen confined to the implementation. The normal equations
are deliberately NOT formed: the solve is a column-equilibrated,
column-pivoted Householder QR, with the cofactor matrix from triangular
solves, which gives the same estimate without squaring the condition number
(this line used to say "normal-equation solve", describing the method the code
rejects). `statistics.cpp` computes
the chi-square CDF through the regularised lower incomplete gamma, using an
exact half-integer ln-Gamma so the 2 and 4 degree-of-freedom cases are correct
to the last place.

114 survey tests. An earlier placeholder that returned hard-coded values was
deleted; that incident is why every numeric expectation in this project must be
justified from an external definition rather than from program output.

**CORRECTED (2026-09-23): the network adjustments had NO tests.**
`adjustHorizontalNetwork`, `adjustLevelNetwork`, `adjustTraverse`,
`buildTraverseNetwork` and both `applyAdjustment` overloads - about 900 lines
of `network_adjustment.cpp` - were referenced by no file under `tests/`, and
never had been (`git log` finds no deleted test). The 114 tests above cover the
dense least-squares core, the traverse computation, levelling runs and the
statistics, not the network solve built on them. `tests/survey/
test_network_adjustment.cpp` now exists with a hand-worked level network.
OUTSTANDING: the horizontal and traverse adjustments still need tests against
published worked examples (Ghilani, *Adjustment Computations*, is the standard
source), with the error ellipses and the global test among what they assert.

---

