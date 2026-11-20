//!
//! The GPU's own time for the calls of the tracer, from timestamp queries, outside of browsers. The time between the
//! frames of an application also holds what the application draws and reads back; on a large window that alone can
//! be longer than the limit of the preview.
//!

use super::CallTimes;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use three_d::wgpu as w;

/// Calls measured at once; a call while all of them are still read back is not measured.
const SLOTS: u32 = 4;

/// Timestamps at the beginning and the end of the calls, where the device has timestamp queries.
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

/// The two timestamps of a call being measured.
#[derive(Clone, Copy, Debug)]
pub(super) struct Stamp {
    slot: u32,
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
            count: 2 * SLOTS,
        });
        let resolved = device.create_buffer(&w::BufferDescriptor {
            label: Some("ray tracing time"),
            size: 16 * SLOTS as u64,
            usage: w::BufferUsages::QUERY_RESOLVE | w::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&w::BufferDescriptor {
                    label: Some("ray tracing time, read back"),
                    size: 16,
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

    /// The timestamps for a call, if a pair is free. The call must go on to [GpuClock::read].
    pub(super) fn begin(&self) -> Option<Stamp> {
        let slot = self
            .slots
            .iter()
            .position(|slot| !slot.busy.swap(true, Ordering::AcqRel))?;
        Some(Stamp { slot: slot as u32 })
    }

    /// The timestamp writes of a pass: the first pass of the call writes the beginning, the last one the end.
    pub(super) fn writes(
        &self,
        stamp: Stamp,
        first: bool,
        last: bool,
    ) -> Option<w::ComputePassTimestampWrites<'_>> {
        (first || last).then(|| w::ComputePassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: first.then_some(2 * stamp.slot),
            end_of_pass_write_index: last.then_some(2 * stamp.slot + 1),
        })
    }

    /// After the last pass of the call, in the same encoder: copies the timestamps to where they are read.
    pub(super) fn resolve(&self, encoder: &mut w::CommandEncoder, stamp: Stamp) {
        let at = 16 * stamp.slot as u64;
        encoder.resolve_query_set(
            &self.queries,
            2 * stamp.slot..2 * stamp.slot + 2,
            &self.resolved,
            at,
        );
        encoder.copy_buffer_to_buffer(
            &self.resolved,
            at,
            &self.slots[stamp.slot as usize].buffer,
            0,
            16,
        );
    }

    ///
    /// After the submission of [GpuClock::resolve]: reads the time once the GPU is done (when the device is polled)
    /// and adds the milliseconds to `times` if they are still in `round`.
    ///
    pub(super) fn read(&self, stamp: Stamp, times: &Arc<Mutex<CallTimes>>, round: u64) {
        let slot = &self.slots[stamp.slot as usize];
        let buffer = slot.buffer.clone();
        let busy = slot.busy.clone();
        let times = times.clone();
        let period = self.period;
        slot.buffer
            .slice(..)
            .map_async(w::MapMode::Read, move |result| {
                if result.is_ok() {
                    let ticks = {
                        let data = buffer.slice(..).get_mapped_range();
                        let word = |i: usize| {
                            let mut bytes = [0; 8];
                            bytes.copy_from_slice(&data[8 * i..8 * i + 8]);
                            u64::from_le_bytes(bytes)
                        };
                        word(1).checked_sub(word(0))
                    };
                    buffer.unmap();
                    if let (Some(ticks @ 1..), Ok(mut times)) = (ticks, times.lock()) {
                        if times.round == round {
                            times.finished.push(ticks as f64 * period / 1e6);
                        }
                    }
                }
                busy.store(false, Ordering::Release);
            });
    }
}
