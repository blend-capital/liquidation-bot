use serde_aux::prelude::deserialize_default_from_null;
use reqwest::{ Client, Error };

#[derive(Debug, serde::Serialize)]
pub struct Request<T> {
    jsonrpc: &'static str,
    id: u32,
    method: String,
    params: T,
}
#[derive(Debug, serde::Serialize)]
pub struct ParamLessRequest {
    jsonrpc: &'static str,
    id: u32,
    method: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct Response<T> {
    result: T,
}
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct ContractEvent {
    #[serde(rename = "type")]
    pub event_type: String,

    pub ledger: u32,
    #[serde(rename = "ledgerClosedAt")]
    pub ledger_closed_at: String,

    pub id: String,
    #[serde(rename = "pagingToken")]
    pub paging_token: String,

    #[serde(rename = "contractId")]
    pub contract_id: String,
    pub topic: Vec<String>,
    pub value: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct GetEventsResponse {
    #[serde(deserialize_with = "deserialize_default_from_null")]
    pub events: Vec<ContractEvent>,
    #[serde(rename = "latestLedger")]
    pub latest_ledger: u32,
}
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub enum EventType {
    All,
    #[serde(rename = "contract")]
    Contract,
    System,
}
#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct EventFilter {
    #[serde(rename = "type")]
    pub event_type: EventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "contractIds")]
    pub contract_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<Vec<String>>>,
}
#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct PaginationFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}
#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct GetEventRequest {
    #[serde(rename = "startLedger")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_ledger: Option<u32>,
    pub filters: Vec<EventFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<PaginationFilter>,
}
#[derive(Clone)]
pub struct Server {
    server_url: String,
    client: reqwest::Client,
}
#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct GetLatestLedgerResponse {
    pub sequence: u32,
}
impl Server {
    pub fn new(server_url: &str) -> Self {
        let client = Client::new();
        return Self { server_url: server_url.to_string(), client };
    }

    pub async fn get_latest_ledger(&self) -> Result<GetLatestLedgerResponse, Error> {
        let request = ParamLessRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "getLatestLedger".to_owned(),
        };
        let res = self.client.post(self.server_url.clone()).json(&request).send().await.unwrap();
        res.error_for_status_ref()?;
        Ok(res.json::<Response<GetLatestLedgerResponse>>().await.unwrap().result)
    }

    pub async fn get_events(&self, params: GetEventRequest) -> Result<GetEventsResponse, Error> {
        let request = Request {
            jsonrpc: "2.0",
            id: 1,
            method: "getEvents".to_owned(),
            params,
        };
        let res = self.client.post(self.server_url.clone()).json(&request).send().await.unwrap();
        res.error_for_status_ref()?;
        Ok(res.json::<Response<GetEventsResponse>>().await.unwrap().result)
    }
}
