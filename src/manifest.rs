use std::path::PathBuf;
use serde::{Serialize, Deserialize};

#[derive(Default, Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Manifest {
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub mcp_servers: Vec<String>,
}
