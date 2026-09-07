// SPDX-License-Identifier: MIT
#pragma once
class Parameters;
namespace chain_position {
void guard(Parameters &);
void startup();
void finish();
void read_begin();
void read_end();
void chain_opportunity(unsigned long long, unsigned long long,
                       unsigned long long, unsigned long long,
                       unsigned long long, unsigned long long);
void reverse_suppressed(unsigned long long, unsigned long long,
                        unsigned long long, unsigned long long,
                        unsigned long long, unsigned long long);
void outer_begin(unsigned long long, unsigned long long, unsigned long long,
                 unsigned long long, unsigned long long, unsigned long long,
                 unsigned long long, unsigned long long, unsigned long long);
void outer_end();
void inner_begin(unsigned long long, unsigned long long, unsigned long long,
                 unsigned long long, bool, unsigned long long);
void inner_end();
void phase(unsigned);
void compare_begin();
void compared(unsigned long long);
} // namespace chain_position
