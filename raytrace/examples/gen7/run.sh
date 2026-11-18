#!/bin/bash
# DIAGNOSIS ONLY: GPU hang of the compute tracer on Intel Gen7 (T430). Run from the repository root:
#   bash raytrace/examples/gen7/run.sh 2>&1 | tee gen7-probe.log
# Every case runs in its own process with a time limit; a GPU hang only ends that process.
# Scratch per thread on Ivy Bridge (Mesa 26.0.8): base 11 KB, nogather 10 KB, noshadow 4 KB, primary 2 KB.
set -u
cargo build --release -p three-d-raytrace --features wgpu,three-d/wgpu-gl --example gen7_probe || exit 1
bin=target/release/examples/gen7_probe
dir=raytrace/examples/gen7
hangs() { (dmesg 2>/dev/null || journalctl -k -b --no-pager 2>/dev/null) | grep -ciE "gpu hang|context reset"; }
for backend in vulkan gl; do
  for variant in primary noshadow nogather base; do
    for case in "1 0" "4 0" "1 1"; do
      before=$(hangs)
      out=$(WGPU_BACKEND=$backend WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER=1 THREE_D_RAYTRACE_TRACE_WGSL=$dir/$variant.wgsl \
        timeout 120 $bin $case 2>&1)
      code=$?
      after=$(hangs)
      result=$(echo "$out" | grep RESULT || echo "$out" | grep -iE "panicked|error|invalid|lost" | head -2 | tr '\n' ' ')
      echo "$backend $variant spc/features=$case exit=$code new_kernel_hang_lines=$((after - before)) $result"
      [ $code -ne 0 ] && sleep 5
    done
  done
done
