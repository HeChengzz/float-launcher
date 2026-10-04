use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub is_dir: bool,
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "file".to_string()
}