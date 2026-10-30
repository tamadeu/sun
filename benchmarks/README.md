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
