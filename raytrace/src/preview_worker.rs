//!
//! Tracing of the CPU preview on a thread of its own, so the frames of the application do not wait for it.
//!

use crate::{
    AtrousDenoiser, Camera, CpuReferenceTracer, Denoiser, RenderImage, RtScene, TraceSettings,
    Tracer,
};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How often the worker hands over an image at most.
const HAND_OVER: Duration = Duration::from_millis(50);

/// What the worker traces.
#[derive(Clone)]
pub(crate) struct Job {
    pub scene: Arc<RtScene>,
    /// The camera with the viewport of the reduced resolution.
    pub camera: Camera,
    pub max_bounces: u32,
    pub caustic_photons: u32,
    pub max_samples: u32,
    pub denoiser: Option<AtrousDenoiser>,
}

/// A finished image of the worker.
pub(crate) struct Finished {
    pub generation: u64,
    pub samples: u32,
    /// Averages with alpha one, top row first.
    pub rgba: Vec<[f32; 4]>,
}

#[derive(Default)]
struct State {
    job: Option<Job>,
    /// Counts the jobs, so an image of an older job is never shown.
    generation: u64,
    finished: Option<Finished>,
    /// A trace failed; the text of the error, once.
    error: Option<crate::RtError>,
    stop: bool,
}

/// The thread and what it shares with the preview.
pub(crate) struct CpuWorker {
    shared: Arc<(Mutex<State>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
    generation: u64,
}

impl CpuWorker {
    /// Starts the thread for images of the given size.
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let shared: Arc<(Mutex<State>, Condvar)> = Arc::default();
        let worker_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("ray tracing preview".to_owned())
            .spawn(move || run(worker_shared, width, height))
            .expect("a thread for the preview");
        Self {
            shared,
            thread: Some(thread),
            generation: 0,
        }
    }

    /// Starts a new job; images of the previous one are dropped.
    pub(crate) fn start(&mut self, job: Job) {
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        state.generation += 1;
        self.generation = state.generation;
        state.job = Some(job);
        state.finished = None;
        wake.notify_all();
    }

    /// Changes the settings of the running job that do not need a new start.
    pub(crate) fn adjust(&self, max_samples: u32, denoiser: Option<AtrousDenoiser>) {
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(job) = &mut state.job {
            if job.max_samples != max_samples || job.denoiser != denoiser {
                job.max_samples = max_samples;
                job.denoiser = denoiser;
                wake.notify_all();
            }
        }
    }

    /// The newest image of the current job, if there is one not taken yet, or the error of a failed trace.
    pub(crate) fn take(&self) -> Result<Option<Finished>, crate::RtError> {
        let (lock, _) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = state.error.take() {
            return Err(error);
        }
        Ok(state
            .finished
            .take()
            .filter(|f| f.generation == self.generation))
    }
}

impl Drop for CpuWorker {
    fn drop(&mut self) {
        {
            let (lock, wake) = &*self.shared;
            lock.lock().unwrap_or_else(|e| e.into_inner()).stop = true;
            wake.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: Arc<(Mutex<State>, Condvar)>, width: u32, height: u32) {
    let (lock, wake) = &*shared;
    let mut tracer = CpuReferenceTracer::new();
    let mut image = RenderImage::with_features(width, height);
    let mut traced = 0;
    let mut hand_over = HAND_OVER;
    loop {
        // Wait for work: a new job, or more samples for the current one
        let (generation, job) = {
            let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if state.stop {
                    return;
                }
                if let Some(job) = &state.job {
                    if state.generation != traced || image.sample_count() < job.max_samples {
                        break;
                    }
                }
                state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            (state.generation, state.job.clone().expect("a job"))
        };
        if generation != traced {
            image.reset();
            traced = generation;
        }
        let settings = TraceSettings {
            max_bounces: job.max_bounces,
            caustic_photons: job.caustic_photons,
            ..Default::default()
        };
        // Samples are added until the time for a hand over has come, so the threads trace instead of waiting for the
        // denoiser and the copy, which take as long as several samples of a small image
        let started = Instant::now();
        let result = loop {
            let result = tracer.trace(&job.scene, &job.camera, &mut image, &settings);
            let first = image.sample_count() == 1;
            if result.is_err()
                || first
                || image.sample_count() >= job.max_samples
                || started.elapsed() >= hand_over
                || is_replaced(lock, generation)
            {
                break result;
            }
        };
        let denoising = Instant::now();
        let result = result.and_then(|()| match job.denoiser {
            Some(mut denoiser) => denoiser.denoise(&image),
            None => Ok(image.clone()),
        });
        // Handing over costs no more than a fifth of the time
        hand_over = (denoising.elapsed() * 4).max(HAND_OVER);
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(shown) if state.generation == generation => {
                state.finished = Some(Finished {
                    generation,
                    samples: image.sample_count(),
                    rgba: shown.rgba(),
                });
            }
            Ok(_) => {}
            Err(error) => {
                state.error = Some(error);
                // Not again until there is a new job
                state.job = None;
            }
        }
    }
}

/// Whether a newer job waits.
fn is_replaced(lock: &Mutex<State>, generation: u64) -> bool {
    let state = lock.lock().unwrap_or_else(|e| e.into_inner());
    state.stop || state.generation != generation
}
