# Allocator baseline — page-per-alloc mmap (pre-mimalloc)

Captured on the milestone-1 std allocator (`std/src/sys/alloc/fullrust.rs`):
every allocation is a page-rounded private `mmap`, freed with `munmap`.
Machine reported 32 CPUs. `cargo +fullrust-1.88 run --release`.

```
[1] single-thread 64B alloc+free: 0.35 M ops/s  (2879.1 ns/op)
[2]  1 threads 48B alloc+free:   0.37 M ops/s  (0.37 M ops/s/thread)
[2]  2 threads 48B alloc+free:   0.19 M ops/s  (0.10 M ops/s/thread)
[2] 32 threads 48B alloc+free:   0.16 M ops/s  (0.01 M ops/s/thread)   <- negative scaling
[3] cross-thread alloc->free:    0.24 M ops/s
[4] fragmentation churn: live=218.4MiB  peak_RSS=312.0MiB  overhead=1.43x  RSS_after_free=1.1MiB
```

Weaknesses: a syscall pair per op (≈2.9 µs), kernel mmap-lock contention kills
multicore scaling, 1.43x RSS overhead. Strength: munmap-on-free returns memory
fully. The mimalloc-class replacement must beat [1]-[3] by 100x+ and cut [4]'s
overhead while keeping RSS-after-free low.
