//! vault-mcp: a read-only MCP server (stdio) for finding and reading old
//! Claude Code sessions without burning tokens.

pub mod frontmatter;
pub mod index;
pub mod rpc;
pub mod sandbox;
pub mod search;
pub mod text;
pub mod tools;
pub mod transcript;

pub use tools::{Config, Server};
