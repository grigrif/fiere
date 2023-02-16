use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
pub struct SubmitPayload {
    pub expire: i64,
    pub name: String,
    pub max_download: Option<usize>,
}
#[derive(Serialize)]
pub struct Status { pub offset: Option<i64> , pub hash: Option<String>,pub file_size: Option<i64>  }

#[derive(Serialize)]
pub struct SubmitResponse {
    pub identifier: Option<String>,
    pub expired_at: Option<i64>,
}

#[derive(Serialize)]
pub struct GetOneFileResponse {
    pub file_size: Option<i64>,
    pub expired_at: Option<i64>,
    pub name: Option<String>,
}
#[derive(Serialize)]

pub struct GetPartsResponse {
    pub file_size: i64,
    pub identifier: String,
    pub hash: String,
    pub offset: i64,
}