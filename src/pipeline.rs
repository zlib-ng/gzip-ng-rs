// pipeline.rs -- ordered submit and drain of slots through the pool
// For conditions of distribution and use, see LICENSE.md

use crate::pool::{Mode, Pool, Slot};

pub struct Pipeline {
    pub pool: Pool,
    pub next_submit: usize,
    pub next_drain: usize,
}

impl Pipeline {
    pub fn start(mode: Mode, nthreads: usize, in_size: usize, out_size: usize) -> Option<Self> {
        Some(Self {
            pool: Pool::start(mode, nthreads, in_size, out_size)?,
            next_submit: 0,
            next_drain: 0,
        })
    }

    /// Whether the next slot to submit is ready to fill.
    pub fn has_free(&self) -> bool {
        self.pool.is_free(self.next_submit)
    }

    /// Take the next slot to submit, which must be free.
    pub fn take(&self) -> Slot {
        self.pool.take(self.next_submit)
    }

    pub fn submit(&mut self, slot: Slot) {
        self.pool.submit(self.next_submit, slot);
        self.next_submit += 1;
    }

    /// The block at next_drain went out in order, the next one is up.
    pub fn drained(&mut self) {
        self.next_drain += 1;
    }

    pub fn has_pending(&self) -> bool {
        self.next_drain < self.next_submit
    }

    pub fn wait(&mut self, index: usize) -> Slot {
        self.pool.wait(index)
    }

    /// Everything submitted but not yet drained was taken back, nothing is pending.
    pub fn clear(&mut self) {
        self.next_submit = self.next_drain;
    }

    pub fn reset(&mut self) {
        self.next_submit = 0;
        self.next_drain = 0;
    }
}
