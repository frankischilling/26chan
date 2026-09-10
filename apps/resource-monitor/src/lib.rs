#![forbid(unsafe_code)]

pub mod collect;
pub mod config;

mod sampler;
pub use sampler::{SAMPLE_PERIOD, SampleState, sample_loop};

#[cfg(test)]
mod tests;
