//! Edges

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::workspace_snapshot::{
    change_set::ChangeSet,
    vector_clock::{VectorClock, VectorClockError},
};

#[derive(Debug, Error)]
pub enum EdgeWeightError {
    #[error("Vector Clock error: {0}")]
    VectorClock(#[from] VectorClockError),
}

pub type EdgeWeightResult<T> = Result<T, EdgeWeightError>;

#[derive(Default, Debug, Serialize, Deserialize, Clone, Copy)]
pub enum EdgeWeightKind {
    #[default]
    Uses,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct EdgeWeight {
    pub kind: EdgeWeightKind,
    pub vector_clock_seen: VectorClock,
    pub vector_clock_write: VectorClock,
}

impl EdgeWeight {
    pub fn new(change_set: &ChangeSet, kind: EdgeWeightKind) -> EdgeWeightResult<Self> {
        Ok(Self {
            kind,
            vector_clock_seen: VectorClock::new(change_set)?,
            vector_clock_write: VectorClock::new(change_set)?,
        })
    }

    pub fn new_with_incremented_vector_clocks(
        &self,
        change_set: &ChangeSet,
    ) -> EdgeWeightResult<Self> {
        let mut new_weight = self.clone();
        new_weight.increment_vector_clocks(change_set)?;

        Ok(new_weight)
    }

    pub fn increment_vector_clocks(&mut self, change_set: &ChangeSet) -> EdgeWeightResult<()> {
        self.vector_clock_seen.inc(change_set)?;
        self.vector_clock_write.inc(change_set)?;

        Ok(())
    }
}
