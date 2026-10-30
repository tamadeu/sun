#include <math.h>
#include <stdio.h>

/*
 * Sun native runtime ABI v1.
 * Stable C boundary used by generated LLVM modules. Matrix storage remains
 * compiler-owned in this milestone; opaque heap-backed matrices are next.
 */
double sun_rt_sqrt(double value) { return sqrt(value); }
double sun_rt_sin(double value)  { return sin(value); }
double sun_rt_cos(double value)  { return cos(value); }
double sun_rt_exp(double value)  { return exp(value); }
double sun_rt_log(double value)  { return log(value); }

void sun_rt_print_f64(double value) { printf("%g\n", value); }
