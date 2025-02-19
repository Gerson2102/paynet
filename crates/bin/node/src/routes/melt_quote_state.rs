use nuts::nut05::MeltQuoteResponse;
use tonic::Status;
use uuid::Uuid;

use crate::{grpc_service::GrpcState, methods::Method};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Db(#[from] memory_db::Error),
}

impl From<Error> for Status {
    fn from(value: Error) -> Self {
        Status::invalid_argument(value.to_string())
    }
}

impl GrpcState {
    pub async fn inner_melt_quote_state(
        &self,
        method: Method,
        quote_id: Uuid,
    ) -> Result<MeltQuoteResponse<Uuid>, Error> {
        match method {
            Method::Starknet => {}
        }

        let mut conn = self.pg_pool.acquire().await?;

        let melt_quote_response =
            memory_db::melt_quote::build_response_from_db(&mut conn, quote_id).await?;

        Ok(melt_quote_response)
    }
}
