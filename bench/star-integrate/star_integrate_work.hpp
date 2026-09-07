#ifndef STAR_INTEGRATE_WORK_HPP
#define STAR_INTEGRATE_WORK_HPP

#include <stdint.h>

// Accounting is a diagnostic build option.  Production timing builds pass
// -DSTAR_INTEGRATE_COUNTERS=0, so every observer call below is an inline no-op.
// Keep it on by default for the existing gate and observer fixtures.
#ifndef STAR_INTEGRATE_COUNTERS
#define STAR_INTEGRATE_COUNTERS 1
#endif

/*
 * This observer accounts only for stock CPU work reached from STAR's original
 * inner maxMappableLength call.  It deliberately has no callback in the
 * comparator's byte loop: the four original return sites report the exact
 * number of compared bytes instead.
 */
namespace star_integrate_work {

enum Arm { ARM_NONE, ARM_FALLBACK, ARM_ORACLE };

#if STAR_INTEGRATE && STAR_INTEGRATE_COUNTERS
class Scope {
public:
  explicit Scope(Arm arm);
  ~Scope();

private:
  Arm previous_;
};

void inner_call(bool hit);
void compare_begin();
void compared(uint64_t bytes);
void finish();

inline Scope fallback_scope() { return Scope(ARM_FALLBACK); }
inline Scope oracle_scope() { return Scope(ARM_ORACLE); }
#else
class Scope {
public:
  explicit Scope(Arm) {}
};
inline void inner_call(bool) {}
inline void compare_begin() {}
inline void compared(uint64_t) {}
inline void finish() {}
inline Scope fallback_scope() { return Scope(ARM_FALLBACK); }
inline Scope oracle_scope() { return Scope(ARM_ORACLE); }
#endif

} // namespace star_integrate_work

#endif
