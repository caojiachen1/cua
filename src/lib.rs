// Shared library surface for perception examples/tests and future helpers.
// The CLI keeps its own module graph; this library exposes the same files so
// external tests and development probes can exercise them without spawning
// the whole CLI.
pub mod action;
pub mod capture;
pub mod input;
pub mod perception;
pub mod uia;
pub mod util;
pub mod win;
