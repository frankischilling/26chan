#![forbid(unsafe_code)]

pub mod config;
pub mod journal;
mod sampler;

pub use sampler::{SAMPLE_PERIOD, SampleState, sample_loop};

#[cfg(test)]
mod tests;
