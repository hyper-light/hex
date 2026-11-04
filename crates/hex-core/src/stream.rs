//! The node agent's wire over a byte stream (docs/design/shards-guest.md §3): the stub its tests fail
//! against, before the core is written.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::wire::{self, Fault};

pub const NAME_BYTES: usize = 16;

pub fn frame(_message: &[u8], _bound: usize) -> Result<Vec<u8>, Fault> {
    Err(Fault::Magic)
}

pub struct Deframer {
    bound: usize,
}

impl Deframer {
    pub fn new(bound: usize) -> Self {
        Self { bound }
    }

    pub fn take(&mut self, _input: &[u8]) -> Result<(usize, Option<Vec<u8>>), Fault> {
        Err(Fault::Magic)
    }

    pub fn at_boundary(&self) -> bool {
        false
    }

    pub fn buffered(&self) -> usize {
        self.bound
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stdio {
    In,
    Out,
    Err,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Named {
    pub exec: u64,
    pub stream: Stdio,
}

pub fn name(_named: Named) -> [u8; NAME_BYTES] {
    [0; NAME_BYTES]
}

pub fn read_name(_bytes: &[u8; NAME_BYTES]) -> Result<Named, Fault> {
    Err(Fault::Magic)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    Twice,
    Unwanted,
    Full { bound: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stray<C> {
    pub conn: C,
    pub why: Why,
}

pub type Complete<C> = (u64, Vec<C>);

pub struct Gather<C> {
    sets: BTreeMap<u64, C>,
}

impl<C> Gather<C> {
    pub fn new(_bound: usize) -> Self {
        Self {
            sets: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn expect(
        &mut self,
        _exec: u64,
        _streams: wire::Streams,
        _now: Instant,
    ) -> Result<Option<Complete<C>>, Why> {
        Err(Why::Twice)
    }

    pub fn arrive(
        &mut self,
        _named: Named,
        conn: C,
        _now: Instant,
    ) -> Result<Option<Complete<C>>, Stray<C>> {
        Err(Stray {
            conn,
            why: Why::Twice,
        })
    }

    pub fn expire(&mut self, _now: Instant, _patience: Duration) -> Vec<Complete<C>> {
        let _ = &self.sets;
        Vec::new()
    }
}

#[cfg(test)]
mod tests;
