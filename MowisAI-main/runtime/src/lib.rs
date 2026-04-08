/// Runtime Crate
///
/// Manages the complete lifecycle of sandboxes and containers.
/// Provides the infrastructure layer that talks to agentd.

pub mod agentd_client;
pub mod runtime;

// Re-export public types for convenience
pub use agentd_client::{AgentdClient, AgentdClientError, AgentdClientResult};
pub use runtime::{Runtime, RuntimeError, RuntimeResult};
