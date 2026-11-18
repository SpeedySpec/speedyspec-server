//! In-memory doubles of the release contracts, for tests, and a builder for `.tar.zst` release archives.

pub mod archive;
pub mod inspector;
pub mod runtime;
pub mod source;
pub mod state;
pub mod verifier;

pub use inspector::FakeInspector;
pub use runtime::RecordingRuntime;
pub use source::MemorySource;
pub use state::MemoryStateStore;
pub use verifier::MemoryVerifier;
