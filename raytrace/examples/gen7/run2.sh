#!/bin/bash
# DIAGNOSIS ONLY, second round: the cases that hung with garden_gpu, with the time of every call.
#   bash raytrace/examples/gen7/run2.sh 2>&1 | tee gen7-probe2.log
set -u
cargo build --release -p three-d-raytrace --features wgpu,three-d/wgpu-gl --example gen7_probe --example garden_gpu || exit 1
hangs() { (journalctl -k -b --no-pager 2>/dev/null || dmesg 2>/dev/null) | grep -ciE "gpu hang|context reset"; }
run() {
  before=$(hangs)
  out=$(WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER=1 PROBE_SYNC=1 timeout 300 "$@" 2>&1)
  code=$?
  echo "== $* | exit=$code new_hang_lines=$(( $(hangs) - before ))"
  echo "$out" | grep -E "RESULT|call |samples after|panicked|invalid|lost|error" | head -20
  [ $code -ne 0 ] && sleep 5
}
for backend in vulkan gl; do
  export WGPU_BACKEND=$backend
  run target/release/examples/gen7_probe 8 0 2 160 100
  run target/release/examples/gen7_probe 8 1 2 160 100
  run target/release/examples/gen7_probe 1 1 2 640 400
  run target/release/examples/gen7_probe 8 1 2 640 400
  run target/release/examples/garden_gpu 8 160 100 probe_small
  run target/release/examples/garden_gpu 64 640 400 probe_large
done
