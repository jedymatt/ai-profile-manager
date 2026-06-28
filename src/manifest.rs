use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Default, Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Manifest {
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub mcp_servers: Vec<String>,
}
