#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RegistryStatus {
    Candidate,
    Approved,
    Disabled,
}

