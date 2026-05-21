pub mod r#loop;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryAgent {
    Build,
    Plan,
}

impl PrimaryAgent {
    pub const ALL: [Self; 2] = [Self::Build, Self::Plan];

    pub fn id(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Plan => "plan",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Plan => "Plan",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Build => "default coding agent for implementation",
            Self::Plan => "read-only analysis agent for planning",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|agent| *agent == self)
            .unwrap_or(0)
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "build" => Some(Self::Build),
            "plan" => Some(Self::Plan),
            _ => None,
        }
    }

    pub fn system_prompt(self) -> &'static str {
        match self {
            Self::Build => {
                "You are the Build agent. Implement requested changes with minimal, safe edits. Prefer concrete execution over long explanations. Keep outputs concise and action-oriented."
            }
            Self::Plan => {
                "You are the Plan agent. Analyze and propose implementation steps only. Do not claim code was changed. Focus on architecture, risks, and a minimal execution plan."
            }
        }
    }
}
