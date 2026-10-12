// pool.rs -- the thread pool and slot ring behind the gzblock engine
// For conditions of distribution and use, see LICENSE.md

use std::collections::VecDeque;
use std::mem;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use crate::buf::Buf;
use crate::codec::{Codec, Status};
use crate::zng::Strategy;

/// Upper bound on block buffers held in flight.
const RING_BYTES: u64 = 1 << 30;

/// One block on its way through the pool. Decompression inflates segments into blocks,
/// compression deflates blocks into pieces.
#[derive(Default)]
pub struct Slot {
    /// Input block or compressed segment, in_len bytes of it, owned by the slot.
    pub input: Vec<u8>,
    pub in_len: usize,
    /// Final piece of the input.
    pub last: bool,
    /// Whole gzip members in the input, 0 for a raw deflate segment.
    pub members: i32,
    /// Grows past the block size for segments that inflate larger.
    pub out: Vec<u8>,
    /// Deflate settings for this block.
    pub level: i32,
    pub strategy: Strategy,
    pub status: Status,
    pub out_len: usize,
    pub in_used: usize,
    /// crc32 of the uncompressed side.
    pub crc: u32,
}

impl Slot {
    /// Move seg into the slot and leave seg the slot's old buffer for its next fill, no copy
    /// either way. The caller sets what the segment means.
    pub fn swap_in(&mut self, seg: &mut Buf) {
        self.in_len = seg.swap(&mut self.input);
    }

    /// Append to the slot's input, which the caller has bounded to what fits.
    pub fn append(&mut self, buf: &[u8]) {
        self.input[self.in_len..self.in_len + buf.len()].copy_from_slice(buf);
        self.in_len += buf.len();
    }
}

#[derive(Clone, Copy)]
pub enum Mode {
    Inflate,
    Deflate { level: i32, strategy: Strategy },
}

/// A place in the ring. A slot is free to fill, filled and waiting for a worker, away with a
/// worker or with the calling thread, or done and waiting to be drained.
enum Cell {
    Free(Slot),
    Filled(Slot),
    Away,
    Done(Slot),
}

/// Ring of blocks, filled in order by the calling thread, worked on by the pool, drained in
/// order.
struct Ring {
    cells: Vec<Cell>,
    /// Filled places in fill order.
    queue: VecDeque<usize>,
    abort: bool,
}

struct Shared {
    /// Bytes in each slot's out buffer.
    out_size: usize,
    ring: Mutex<Ring>,
    /// A slot was queued, or abort.
    work_cv: Condvar,
    /// A slot became done.
    done_cv: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct Pool {
    shared: Arc<Shared>,
    nring: usize,
    threads: Vec<JoinHandle<()>>,
    /// Stream for working slots on the calling thread, when there are no worker threads and
    /// slots are worked on demand.
    inline: Option<Codec>,
}

fn worker(shared: &Shared, mut codec: Codec) {
    loop {
        let mut ring = shared.lock();
        while !ring.abort && ring.queue.is_empty() {
            ring = shared.work_cv.wait(ring).unwrap_or_else(PoisonError::into_inner);
        }
        if ring.abort {
            break;
        }
        let Some(i) = ring.queue.pop_front() else {
            continue;
        };
        let Cell::Filled(mut slot) = mem::replace(&mut ring.cells[i], Cell::Away) else {
            unreachable!("a queued slot is a filled one");
        };
        drop(ring);

        codec.run(&mut slot, shared.out_size);

        let mut ring = shared.lock();
        ring.cells[i] = Cell::Done(slot);
        shared.done_cv.notify_all();
    }
}

impl Pool {
    pub fn default_threads() -> usize {
        if cfg!(feature = "threads") {
            thread::available_parallelism().map_or(1, |n| n.get().min(64))
        } else {
            1
        }
    }

    /// Allocate the ring, nthreads * 4 slots of in_size + out_size bytes, within RING_BYTES, and
    /// start the workers. One worker runs on the calling thread, with no threads at all.
    pub fn start(mode: Mode, nthreads: usize, in_size: usize, out_size: usize) -> Option<Self> {
        let nthreads = if cfg!(feature = "threads") { nthreads } else { 1 };
        let mut nring = if nthreads <= 1 { 1 } else { nthreads * 4 };

        while nring > 2 && nring as u64 * (in_size as u64 + out_size as u64) > RING_BYTES {
            nring /= 2;
        }
        let cells = (0..nring)
            .map(|_| {
                Cell::Free(Slot {
                    input: vec![0; in_size],
                    out: vec![0; out_size],
                    ..Slot::default()
                })
            })
            .collect();
        let mut pool = Self {
            shared: Arc::new(Shared {
                out_size,
                ring: Mutex::new(Ring {
                    cells,
                    queue: VecDeque::with_capacity(nring),
                    abort: false,
                }),
                work_cv: Condvar::new(),
                done_cv: Condvar::new(),
            }),
            nring,
            threads: Vec::new(),
            inline: None,
        };

        if nthreads <= 1 {
            pool.inline = Some(Codec::new(mode)?);
            return Some(pool);
        }
        for _ in 0..nthreads {
            let shared = Arc::clone(&pool.shared);
            // Made here, where a stream that cannot be had still fails the start.
            let codec = Codec::new(mode)?;
            match thread::Builder::new().spawn(move || worker(&shared, codec)) {
                Ok(thread) => pool.threads.push(thread),
                Err(_) => break,
            }
        }
        if pool.threads.is_empty() {
            return None;
        }
        Some(pool)
    }

    /// Whether place i holds a slot ready to fill.
    pub fn is_free(&self, i: usize) -> bool {
        matches!(self.shared.lock().cells[i % self.nring], Cell::Free(_))
    }

    /// Take the free slot at place i to fill.
    pub fn take(&self, i: usize) -> Slot {
        let Cell::Free(slot) = mem::replace(&mut self.shared.lock().cells[i % self.nring], Cell::Away) else {
            unreachable!("only a free slot is taken to fill");
        };
        slot
    }

    pub fn submit(&self, i: usize, slot: Slot) {
        let i = i % self.nring;
        let mut ring = self.shared.lock();

        ring.cells[i] = Cell::Filled(slot);
        if self.inline.is_none() {
            ring.queue.push_back(i);
            self.shared.work_cv.notify_one();
        }
    }

    /// Wait for the slot at place i to be done and take it.
    pub fn wait(&mut self, i: usize) -> Slot {
        let i = i % self.nring;
        let mut ring = self.shared.lock();

        if let Some(codec) = &mut self.inline {
            // Without worker threads the slots are worked on demand by the calling thread.
            return match mem::replace(&mut ring.cells[i], Cell::Away) {
                Cell::Filled(mut slot) => {
                    drop(ring);
                    codec.run(&mut slot, self.shared.out_size);
                    slot
                }
                Cell::Done(slot) => slot,
                _ => unreachable!("only a submitted slot is waited on"),
            };
        }
        loop {
            if matches!(ring.cells[i], Cell::Done(_)) {
                let Cell::Done(slot) = mem::replace(&mut ring.cells[i], Cell::Away) else {
                    unreachable!();
                };
                return slot;
            }
            ring = self.shared.done_cv.wait(ring).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Take the most recent submission back before any worker claims it. Returns true with the
    /// slot done and untouched, false when a worker already has it.
    pub fn cancel(&self, i: usize) -> bool {
        let i = i % self.nring;
        let mut ring = self.shared.lock();

        // Only the newest queued slot can come back, so the queue stays contiguous.
        if !matches!(ring.cells[i], Cell::Filled(_)) || (self.inline.is_none() && ring.queue.back() != Some(&i)) {
            return false;
        }
        ring.queue.pop_back();
        if let Cell::Filled(slot) = mem::replace(&mut ring.cells[i], Cell::Away) {
            ring.cells[i] = Cell::Done(slot);
        }
        true
    }

    /// Put a slot taken by wait() back as it was, done and waiting to be drained.
    pub fn restore(&self, i: usize, slot: Slot) {
        self.shared.lock().cells[i % self.nring] = Cell::Done(slot);
    }

    /// Hand a drained slot back to be filled again.
    pub fn release(&self, i: usize, slot: Slot) {
        self.shared.lock().cells[i % self.nring] = Cell::Free(slot);
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.lock().abort = true;
        self.shared.work_cv.notify_all();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
