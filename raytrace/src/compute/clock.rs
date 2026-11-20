//!
//! The GPU's own time for the calls of the tracer, from timestamp queries. The time between the frames of an
//! application also holds what the application draws and reads back; on a large window that alone can be longer than
//! the limit of the preview. Browsers tell when work is done only once the page's thread is free, after a frame of the
//! page, which can take longer than the work.
//!

use super::{CallTime, CallTimes};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use three_d::wgpu as w;

/// Calls measured at once; a call while all of them are still read back is not measured.
const SLOTS: u32 = 4;

/// Passes of a call measured one by one; a call with more passes measures groups of consecutive passes.
const PASSES: u32 = 32;

/// Bytes between the resolved timestamps of two calls: queries resolve only to offsets of this alignment.
const SLOT_BYTES: u64 = (2 * PASSES as u64 * 8).next_multiple_of(w::QUERY_RESOLVE_BUFFER_ALIGNMENT);

///
/// Timestamps at the beginning and the end of the passes of the calls, where the device has timestamp queries. Each
/// pass (or group of passes) is timed on its own and the times are added: between the submissions of a call the GPU
/// may work for other threads of the application, the browser or other programs, which is not the tracer's time.
///
#[derive(Debug)]
pub(super) struct GpuClock {
    queries: w::QuerySet,
    resolved: w::Buffer,
    slots: Vec<Slot>,
    /// Nanoseconds per tick of the timestamps
    period: f64,
}

#[derive(Debug)]
struct Slot {
    buffer: w::Buffer,
    /// Taken by a call until its time is read
    busy: Arc<AtomicBool>,
}

/// The timestamps of a call being measured.
#[derive(Clone, Copy, Debug)]
pub(super) struct Stamp {
    slot: u32,
    /// Passes of the call, and passes in each timed group
    passes: u32,
    group: u32,
}

impl Stamp {
    /// Groups of passes timed.
    fn groups(&self) -> u32 {
        self.passes.div_ceil(self.group)
    }
}

impl GpuClock {
    /// A clock for the device, if it has timestamp queries.
    pub(super) fn new(device: &w::Device, queue: &w::Queue) -> Option<Self> {
        if !device.features().contains(w::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let queries = device.create_query_set(&w::QuerySetDescriptor {
            label: Some("ray tracing time"),
            ty: w::QueryType::Timestamp,
            count: 2 * PASSES * SLOTS,
        });
        let resolved = device.create_buffer(&w::BufferDescriptor {
            label: Some("ray tracing time"),
            size: SLOT_BYTES * SLOTS as u64,
            usage: w::BufferUsages::QUERY_RESOLVE | w::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&w::BufferDescriptor {
                    label: Some("ray tracing time, read back"),
                    size: SLOT_BYTES,
                    usage: w::BufferUsages::MAP_READ | w::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                busy: Arc::default(),
            })
            .collect();
        Some(Self {
            queries,
            resolved,
            slots,
            period: queue.get_timestamp_period() as f64,
        })
    }

    /// The timestamps for a call of `passes` passes, if a slot is free. The call must go on to [GpuClock::read].
    pub(super) fn begin(&self, passes: u32) -> Option<Stamp> {
        if passes == 0 {
            return None;
        }
        let slot = self
            .slots
            .iter()
            .position(|slot| !slot.busy.swap(true, Ordering::AcqRel))?;
        Some(Stamp {
            slot: slot as u32,
            passes,
            group: passes.div_ceil(PASSES),
        })
    }

    /// The timestamp writes of the pass with this index in the call: the first pass of a group writes its beginning,
    /// the last one its end.
    pub(super) fn writes(
        &self,
        stamp: Stamp,
        pass: u32,
    ) -> Option<w::ComputePassTimestampWrites<'_>> {
        if pass >= stamp.passes {
            return None;
        }
        let at = 2 * (stamp.slot * PASSES + pass / stamp.group);
        let first = pass.is_multiple_of(stamp.group);
        let last = pass % stamp.group == stamp.group - 1 || pass + 1 == stamp.passes;
        (first || last).then(|| w::ComputePassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: first.then_some(at),
            end_of_pass_write_index: last.then_some(at + 1),
        })
    }

    /// After the last pass of the call, in the same encoder: copies the timestamps to where they are read.
    pub(super) fn resolve(&self, encoder: &mut w::CommandEncoder, stamp: Stamp) {
        let at = SLOT_BYTES * stamp.slot as u64;
        let first = 2 * stamp.slot * PASSES;
        encoder.resolve_query_set(
            &self.queries,
            first..first + 2 * stamp.groups(),
            &self.resolved,
            at,
        );
        encoder.copy_buffer_to_buffer(
            &self.resolved,
            at,
            &self.slots[stamp.slot as usize].buffer,
            0,
            16 * stamp.groups() as u64,
        );
    }

    ///
    /// After the submission of [GpuClock::resolve]: reads the time once the GPU is done (when the device is polled, in
    /// browsers when the browser tells) and adds the milliseconds of the call with its `tiles` to `times` if they are
    /// still in `round`.
    ///
    pub(super) fn read(&self, stamp: Stamp, times: &Arc<Mutex<CallTimes>>, round: u64, tiles: u32) {
        let slot = &self.slots[stamp.slot as usize];
        let buffer = slot.buffer.clone();
        let busy = slot.busy.clone();
        let times = times.clone();
        let period = self.period;
        let bytes = 16 * stamp.groups() as u64;
        slot.buffer
            .slice(..bytes)
            .map_async(w::MapMode::Read, move |result| {
                if result.is_ok() {
                    let ticks = {
                        let data = buffer.slice(..bytes).get_mapped_range();
                        let word = |i: usize| {
                            let mut bytes = [0; 8];
                            bytes.copy_from_slice(&data[8 * i..8 * i + 8]);
                            u64::from_le_bytes(bytes)
                        };
                        // The passes' own times added up
                        (0..bytes as usize / 16).try_fold(0u64, |sum, g| {
                            Some(sum + word(2 * g + 1).checked_sub(word(2 * g))?)
                        })
                    };
                    buffer.unmap();
                    if let (Some(ticks @ 1..), Ok(mut times)) = (ticks, times.lock()) {
                        if times.round == round {
                            times.finished.push(CallTime {
                                ms: ticks as f64 * period / 1e6,
                                tiles,
                            });
                        }
                    }
                }
                busy.store(false, Ordering::Release);
            });
    }
}
