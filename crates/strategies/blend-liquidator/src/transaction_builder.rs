use std::str::FromStr;

use ed25519_dalek::SigningKey;
use soroban_cli::rpc::Client;
use soroban_spec_tools::from_string_primitive;
use stellar_strkey::{ed25519::PublicKey as Ed25519PublicKey, Strkey};
use stellar_xdr::curr::{
    AccountId, Error, Hash, Int128Parts, InvokeContractArgs, InvokeHostFunctionOp, Memo,
    MuxedAccount, Operation, Preconditions, PublicKey, ScAddress, ScMap, ScMapEntry, ScSpecTypeDef,
    ScSymbol, ScVal, ScVec, Transaction, Uint256, VecM,
};
pub struct BlendTxBuilder {
    pub rpc: Client,
}

pub struct Request {
    pub request_type: u32,
    pub address: Hash,
    pub amount: i128,
}

impl BlendTxBuilder {
    pub async fn submit(
        &self,
        pool_id: Hash,
        from: Hash,
        to: Hash,
        spender: Hash,
        requests: Vec<Request>,
        signing_key: SigningKey,
    ) -> Result<Transaction, Error> {
        let op = Operation {
            source_account: None,
            body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                    InvokeContractArgs {
                        contract_address: ScAddress::Contract(pool_id),
                        function_name: ScSymbol::try_from("submit").unwrap(),
                        args: VecM::try_from(vec![
                            ScVal::Address(ScAddress::Account(AccountId(
                                PublicKey::PublicKeyTypeEd25519(Uint256(from.0)),
                            ))),
                            ScVal::Address(ScAddress::Account(AccountId(
                                PublicKey::PublicKeyTypeEd25519(Uint256(spender.0)),
                            ))),
                            ScVal::Address(ScAddress::Account(AccountId(
                                PublicKey::PublicKeyTypeEd25519(Uint256(to.0)),
                            ))),
                            ScVal::Vec(Some(requests_to_scvec(requests))),
                        ])
                        .unwrap(),
                    },
                ),
                auth: VecM::default(),
            }),
        };
        let account = self
            .rpc
            .get_account(
                &Strkey::PublicKeyEd25519(Ed25519PublicKey(signing_key.verifying_key().to_bytes()))
                    .to_string(),
            )
            .await
            .unwrap();
        let seq_num: i64 = account.seq_num.into();
        let transaction: Transaction = Transaction {
            source_account: MuxedAccount::Ed25519(Uint256(signing_key.verifying_key().to_bytes())),
            fee: 10000,
            seq_num: stellar_xdr::curr::SequenceNumber(seq_num + 1), //TODO: we might want to track this client size to remove the need for a account call and to allow us to submit multiple actions in one block
            cond: Preconditions::None,
            memo: Memo::None,
            operations: vec![op].try_into()?,
            ext: stellar_xdr::curr::TransactionExt::V0,
        };
        return Ok(transaction);
    }

    pub async fn new_auction(
        &self,
        pool_id: Hash,
        user: Hash,
        auction_type: u32,
        signing_key: SigningKey,
    ) -> Result<Transaction, Error> {
        let op = Operation {
            source_account: None,
            body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                    InvokeContractArgs {
                        contract_address: ScAddress::Contract(pool_id),
                        function_name: ScSymbol::try_from("new_auction").unwrap(),
                        args: VecM::try_from(vec![
                            ScVal::U32(auction_type),
                            ScVal::Address(ScAddress::Account(AccountId(
                                PublicKey::PublicKeyTypeEd25519(Uint256(user.0)),
                            ))),
                        ])
                        .unwrap(),
                    },
                ),
                auth: VecM::default(),
            }),
        };
        let account = self
            .rpc
            .get_account(
                &Strkey::PublicKeyEd25519(Ed25519PublicKey(signing_key.verifying_key().to_bytes()))
                    .to_string(),
            )
            .await
            .unwrap();
        let seq_num: i64 = account.seq_num.into();
        let transaction: Transaction = Transaction {
            source_account: MuxedAccount::Ed25519(Uint256(signing_key.verifying_key().to_bytes())),
            fee: 10000,
            seq_num: stellar_xdr::curr::SequenceNumber(seq_num + 1),
            cond: Preconditions::None,
            memo: Memo::None,
            operations: vec![op].try_into()?,
            ext: stellar_xdr::curr::TransactionExt::V0,
        };
        return Ok(transaction);
    }
    pub async fn new_liquidation_auction(
        &self,
        pool_id: Hash,
        user: Hash,
        percent_liquidated: u64,

        signing_key: SigningKey,
    ) -> Result<Transaction, Error> {
        let op = Operation {
            source_account: None,
            body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                    InvokeContractArgs {
                        contract_address: ScAddress::Contract(pool_id),
                        function_name: ScSymbol::try_from("new_liquidation_auction").unwrap(),
                        args: VecM::try_from(vec![
                            ScVal::Address(ScAddress::Account(AccountId(
                                PublicKey::PublicKeyTypeEd25519(Uint256(user.0)),
                            ))),
                            ScVal::U64(percent_liquidated),
                        ])
                        .unwrap(),
                    },
                ),
                auth: VecM::default(),
            }),
        };
        let account = self
            .rpc
            .get_account(
                &Strkey::PublicKeyEd25519(Ed25519PublicKey(signing_key.verifying_key().to_bytes()))
                    .to_string(),
            )
            .await
            .unwrap();
        let seq_num: i64 = account.seq_num.into();
        let transaction: Transaction = Transaction {
            source_account: MuxedAccount::Ed25519(Uint256(signing_key.verifying_key().to_bytes())),
            fee: 10000,
            seq_num: stellar_xdr::curr::SequenceNumber(seq_num + 1),
            cond: Preconditions::None,
            memo: Memo::None,
            operations: vec![op].try_into()?,
            ext: stellar_xdr::curr::TransactionExt::V0,
        };
        return Ok(transaction);
    }
}

fn requests_to_scvec(requests: Vec<Request>) -> ScVec {
    let mut vec = Vec::default();
    for request in requests.iter() {
        let map = ScVal::Map(Some(ScMap(
            VecM::try_from(vec![
                ScMapEntry {
                    key: from_string_primitive("address", &ScSpecTypeDef::Symbol).unwrap(),
                    val: ScVal::Address(ScAddress::Contract(request.address.clone())),
                },
                ScMapEntry {
                    key: from_string_primitive("amount", &ScSpecTypeDef::Symbol).unwrap(),
                    val: from_string_primitive(
                        request.amount.to_string().as_str(),
                        &ScSpecTypeDef::I128,
                    )
                    .unwrap(),
                },
                ScMapEntry {
                    key: from_string_primitive("request_type", &ScSpecTypeDef::Symbol).unwrap(),
                    val: ScVal::U32(request.request_type),
                },
            ])
            .unwrap(),
        )));
        vec.push(map.clone());
    }
    ScVec::try_from(vec).unwrap()
}
