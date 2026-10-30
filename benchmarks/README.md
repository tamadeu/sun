# Matrix multiplication benchmarks

The native C runtime uses a cache-blocked, row-major matrix multiplication kernel. The current tile size is 32. The inner column loop traverses contiguous values in the right-hand input and output rows; the reduction index advances in ascending order for each output element.

## Build and run

From the repository root:

```sh
clang -O3 -std=c11 -Wall -Wextra benchmarks/matrix_multiply.c -lm -o matrix-bench
./matrix-bench 5
```

The optional argument is the number of timed repetitions (default: 3). The harness tests square matrices of 31, 32, 33, 64, 128, 256, and 512 elements per side. The sizes around 32 exercise partial tiles and boundary handling.

## What is measured

For each size, the harness:
- Fills deterministic input matrices.
- Computes a reference result with the straightforward i-j-k implementation.
- Compares the blocked kernel against that reference using a maximum absolute error tolerance.
- Times repeated runs of both implementations.
- Prints average elapsed seconds, estimated GFLOP/s for each blocked run, speedup, checksum, and maximum absolute error.

The benchmark includes allocation time in both implementations. It is intended for comparative development measurements, not as a cross-machine performance guarantee. Run it in a quiet environment, use an optimized compiler build, and compare results on the same machine/compiler configuration. The CI job runs one repetition as a correctness and build smoke test; CI timings are not treated as performance evidence.

No speedup is assumed or promised. Tile size and future kernel variants should be selected using measurements across representative hardware and matrix shapes.

## Elementwise kernel benchmark

The runtime's matrix addition, elementwise multiplication, scalar multiplication, and scalar division loops expose independent contiguous buffers and use an OpenMP SIMD loop directive. This is a compiler vectorization hint; it does not require linking the OpenMP runtime. Whether the compiler emits SIMD instructions and whether that improves performance depends on compiler flags and target hardware.

Build and run the elementwise benchmark:

```sh
clang -O3 -std=c11 -Wall -Wextra benchmarks/elementwise_kernels.c -lm -o elementwise-bench
./elementwise-bench 10
```

It checks correctness and compares a scalar reference loop with the runtime operation for tiny, irregular, tile-boundary, and larger matrix shapes. Timings include allocation/free on both paths and are intended as same-machine comparative measurements only. The reported ratio is scalar-loop time divided by runtime-kernel time; values above 1 indicate the runtime path was faster in that particular run. The CI invocation is a correctness/build smoke test, not a performance claim.

## Runtime SIMD dispatch

The elementwise runtime kernels use runtime CPU feature detection on x86. They select AVX-512F when available, otherwise AVX2, and otherwise the portable scalar implementation. Architecture-specific functions use compiler target attributes, so the rest of the runtime is compiled for the baseline target and does not require global AVX compiler flags. Non-x86 platforms always use the portable implementation. The elementwise benchmark validates numerical results for all four operations and a range of matrix shapes; its timings are diagnostic only and do not guarantee that a wider ISA is faster on every workload.
