mod common;
mod evidence;
mod verify;

pub use evidence::{EvidenceItem, EvidenceKind, EvidenceSet, EvidenceStrength};
pub use verify::{GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence};
