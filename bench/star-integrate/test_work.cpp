/* Mock-data observer test; it is not GPU evidence. */
#include "star_integrate_work.hpp"

#include <assert.h>
#include <thread>

static void fallback_mismatch() {
  star_integrate_work::Scope scope = star_integrate_work::fallback_scope();
  star_integrate_work::compare_begin(); // SA decode / gather
  star_integrate_work::compared(3);     // original mismatch return: ii + 1
}

static void oracle_exact() {
  star_integrate_work::Scope scope = star_integrate_work::oracle_scope();
  star_integrate_work::compare_begin(); // SA decode / gather
  star_integrate_work::compared(5);     // original exact return: N - L
}

int main() {
  star_integrate_work::inner_call(false);
  fallback_mismatch();
  star_integrate_work::inner_call(true);
  oracle_exact();
  std::thread worker([] {
    star_integrate_work::inner_call(false);
    fallback_mismatch();
  });
  worker.join(); // exercises TLS retirement merge before main-thread finish
  star_integrate_work::finish();
  return 0;
}
