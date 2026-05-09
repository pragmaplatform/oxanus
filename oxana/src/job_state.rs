use serde::{Serialize, de::DeserializeOwned};

use crate::{JobId, OxanaError, Storage};

#[derive(Clone)]
pub struct JobState {
    storage: Storage,
    job_id: JobId,
    value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JobProgress {
    pub cursor: i64,
    pub total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl From<i64> for JobProgress {
    fn from(cursor: i64) -> Self {
        Self {
            cursor,
            total: 0,
            note: None,
        }
    }
}

impl From<(i64, i64)> for JobProgress {
    fn from((cursor, total): (i64, i64)) -> Self {
        Self {
            cursor,
            total,
            note: None,
        }
    }
}

impl From<(i64, i64, String)> for JobProgress {
    fn from((cursor, total, note): (i64, i64, String)) -> Self {
        Self {
            cursor,
            total,
            note: Some(note),
        }
    }
}

impl From<(i64, i64, Option<String>)> for JobProgress {
    fn from((cursor, total, note): (i64, i64, Option<String>)) -> Self {
        Self {
            cursor,
            total,
            note,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum JobProgressRepr {
    Struct {
        cursor: i64,
        #[serde(default)]
        total: i64,
        #[serde(default)]
        note: Option<String>,
    },
    Tuple3WithNote(i64, i64, Option<String>),
    Tuple2(i64, i64),
    Cursor(i64),
}

impl<'de> serde::Deserialize<'de> for JobProgress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match JobProgressRepr::deserialize(deserializer)? {
            JobProgressRepr::Struct {
                cursor,
                total,
                note,
            }
            | JobProgressRepr::Tuple3WithNote(cursor, total, note) => Self {
                cursor,
                total,
                note,
            },
            JobProgressRepr::Tuple2(cursor, total) => Self {
                cursor,
                total,
                note: None,
            },
            JobProgressRepr::Cursor(cursor) => Self::from(cursor),
        })
    }
}

impl JobState {
    pub(crate) fn new(storage: Storage, job_id: JobId, value: Option<serde_json::Value>) -> Self {
        Self {
            storage,
            job_id,
            value,
        }
    }

    pub async fn update(&self, state: impl Serialize) -> Result<(), OxanaError> {
        self.storage
            .internal
            .update_state(
                &self.job_id,
                serde_json::to_value(state).map_err(OxanaError::JobStateJsonError)?,
            )
            .await?;
        Ok(())
    }

    pub async fn update_progress(
        &self,
        progress: impl Into<JobProgress>,
    ) -> Result<(), OxanaError> {
        self.update(progress.into()).await
    }

    pub async fn progress(&self) -> Result<Option<JobProgress>, OxanaError> {
        Ok(match self.storage.get_job(&self.job_id).await? {
            Some(job) => match job.meta.state {
                Some(state) => {
                    Some(serde_json::from_value(state).map_err(OxanaError::JobStateJsonError)?)
                }
                None => None,
            },
            None => None,
        })
    }

    pub async fn get<S: DeserializeOwned>(&self) -> Result<Option<S>, OxanaError> {
        Ok(match self.value.clone() {
            Some(state) => {
                Some(serde_json::from_value(state).map_err(OxanaError::JobStateJsonError)?)
            }
            None => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::JobProgress;
    use serde_json::json;

    #[test]
    fn job_progress_converts_from_supported_values() {
        assert_eq!(
            JobProgress::from(7),
            JobProgress {
                cursor: 7,
                total: 0,
                note: None,
            }
        );
        assert_eq!(
            JobProgress::from((7, 10)),
            JobProgress {
                cursor: 7,
                total: 10,
                note: None,
            }
        );
        assert_eq!(
            JobProgress::from((7, 10, "chunk imported".to_string())).note,
            Some("chunk imported".to_string())
        );
        assert_eq!(
            JobProgress::from((7, 10, Some("chunk imported".to_string()))).note,
            Some("chunk imported".to_string())
        );
    }

    #[test]
    fn job_progress_deserializes_from_supported_shapes() {
        let expected = JobProgress {
            cursor: 7,
            total: 10,
            note: None,
        };

        assert_eq!(
            serde_json::from_value::<JobProgress>(json!(7)).unwrap(),
            JobProgress::from(7)
        );
        assert_eq!(
            serde_json::from_value::<JobProgress>(json!([7, 10])).unwrap(),
            expected
        );
        assert_eq!(
            serde_json::from_value::<JobProgress>(json!([7, 10, "chunk imported"])).unwrap(),
            JobProgress {
                note: Some("chunk imported".to_string()),
                ..expected.clone()
            }
        );
        assert_eq!(
            serde_json::from_value::<JobProgress>(serde_json::to_value(expected.clone()).unwrap())
                .unwrap(),
            expected
        );
    }
}
