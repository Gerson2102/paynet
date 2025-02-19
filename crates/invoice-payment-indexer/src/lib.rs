use std::ops::DerefMut;
use std::task::Poll;

use apibara_core::node::v1alpha2::DataFinality;
use apibara_core::starknet::v1alpha2::{Block, FieldElement, Filter, HeaderFilter};
use apibara_sdk::{ClientBuilder, Configuration, DataMessage, Uri};
use cashu_starknet::StarknetU256;
use futures::StreamExt;
use rusqlite::Connection;
use starknet_core::types::Felt;
use thiserror::Error;

mod db;

const INVOICE_PAYMENT_CONTRACT_ADDRESS: &str =
    "0x03a94f47433e77630f288054330fb41377ffcc49dacf56568eeba84b017aa633";
const REMITTANCE_EVENT_KEY: &str =
    "0x027a12f554d018764f982295090da45b4ff0734785be0982b62c329b9ac38033";

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid value for field element: {0}")]
    InvalidFieldElement(String),
    #[error("DNA client error")]
    ApibaraClient,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

pub struct ApibaraIndexerService {
    stream: apibara_sdk::ImmutableDataStream<Block>,
    db_conn: Connection,
}

impl Unpin for ApibaraIndexerService {}

impl ApibaraIndexerService {
    pub async fn init(
        mut db_conn: Connection,
        apibara_bearer_token: String,
        target_asset_and_recipient_pairs: Vec<(Felt, Felt)>,
    ) -> Result<Self, Error> {
        db::create_tables(&mut db_conn)?;

        let config = Configuration::<Filter>::default()
            .with_starting_block(458_645)
            .with_finality(DataFinality::DataStatusAccepted)
            .with_filter(|mut filter| {
                let invoice_payment_contract_address =
                    FieldElement::from_hex(INVOICE_PAYMENT_CONTRACT_ADDRESS).unwrap();
                let remittance_event_key = FieldElement::from_hex(REMITTANCE_EVENT_KEY).unwrap();

                target_asset_and_recipient_pairs
                    .iter()
                    .for_each(|(recipient, asset)| {
                        filter
                            .with_header(HeaderFilter::weak())
                            .add_event(|event| {
                                event
                                    .with_from_address(invoice_payment_contract_address.clone())
                                    .with_keys(vec![
                                        remittance_event_key.clone(),
                                        FieldElement::from_hex(&recipient.to_hex_string()).unwrap(),
                                        FieldElement::from_hex(&asset.to_hex_string()).unwrap(),
                                    ])
                            })
                            .build();
                    });

                filter
            });

        let uri = Uri::from_static("https://sepolia.starknet.a5a.ch");
        let stream = ClientBuilder::default()
            .with_bearer_token(Some(apibara_bearer_token))
            .connect(uri)
            .await
            .map_err(|_| Error::ApibaraClient)?
            .start_stream_immutable::<Filter, Block>(config)
            .await
            .map_err(|_| Error::ApibaraClient)?;

        Ok(Self { stream, db_conn })
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Payment(Vec<PaymentEvent>),
    Invalidate {
        last_valid_block_number: u64,
        last_valid_block_hash: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct PaymentEvent {
    pub asset: Felt,
    pub invoice_id: u128,
    pub amount: StarknetU256,
}

impl futures::Stream for ApibaraIndexerService {
    type Item = Result<Message, Box<dyn std::error::Error + Send + Sync + 'static>>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let s = self.deref_mut();

        match s.stream.poll_next_unpin(cx) {
            Poll::Ready(Some(res)) => match res {
                Ok(message) => match message {
                    DataMessage::Data {
                        cursor: _cursor,
                        end_cursor: _end_cursor,
                        finality: _finality,
                        batch,
                    } => {
                        let tx = match s.db_conn.transaction() {
                            Ok(tx) => tx,
                            Err(e) => return Poll::Ready(Some(Err(Box::new(e)))),
                        };

                        let mut payment_events = Vec::with_capacity(batch.len());

                        for block in batch.iter() {
                            let block_infos = block.header.as_ref().unwrap().into();
                            db::insert_new_block(&tx, &block_infos)?;

                            for event in block.events.iter() {
                                let payment_event = match event.event.as_ref().unwrap().try_into() {
                                    Ok(pe) => pe,
                                    Err(e) => return Poll::Ready(Some(Err(Box::new(e)))),
                                };
                                db::insert_payment_event(&tx, &block_infos.id, &payment_event)?;
                                payment_events.push(PaymentEvent {
                                    asset: Felt::from_hex_unchecked(&payment_event.asset),
                                    invoice_id: u128::from_str_radix(
                                        &payment_event.invoice_id[2..],
                                        16,
                                    )
                                    .unwrap(),
                                    amount: StarknetU256::from_parts(
                                        u128::from_str_radix(&payment_event.amount_low[2..], 16)
                                            .unwrap(),
                                        u128::from_str_radix(&payment_event.amount_high[2..], 16)
                                            .unwrap(),
                                    ),
                                });
                            }
                        }

                        match tx.commit() {
                            Ok(()) => Poll::Ready(Some(Ok(Message::Payment(payment_events)))),
                            Err(e) => Poll::Ready(Some(Err(Box::new(e)))),
                        }
                    }
                    DataMessage::Invalidate { cursor } => {
                        let cursor = cursor.unwrap();
                        match db::invalidate(&s.db_conn, cursor.order_key) {
                            Ok(_) => Poll::Ready(Some(Ok(Message::Invalidate {
                                last_valid_block_number: cursor.order_key,
                                last_valid_block_hash: cursor.unique_key,
                            }))),
                            Err(e) => Poll::Ready(Some(Err(Box::new(e)))),
                        }
                    }
                    DataMessage::Heartbeat => Poll::Pending,
                },
                Err(e) => Poll::Ready(Some(Err(e.into()))),
            },
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
