use std::str::FromStr;

use ed25519_dalek::SigningKey;
use soroban_spec_tools::from_string_primitive;
use stellar_xdr::curr::{
    Hash, InvokeContractArgs, InvokeHostFunctionOp, Operation, ScAddress, ScMap, ScMapEntry,
    ScSpecTypeDef, ScSymbol, ScVal, ScVec, VecM,
};
pub struct BlendTxBuilder {
    pub contract_id: Hash,
    pub signing_key: SigningKey,
}

pub struct Request {
    pub request_type: u32,
    pub address: String,
    pub amount: i128,
}

impl BlendTxBuilder {
    pub fn submit(&self, from: &str, to: &str, spender: &str, requests: Vec<Request>) -> Operation {
        Operation {
            source_account: None,
            body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                    InvokeContractArgs {
                        contract_address: ScAddress::Contract(self.contract_id.clone()),
                        function_name: ScSymbol::try_from("submit").unwrap(),
                        args: VecM::try_from(vec![
                            ScVal::Address(ScAddress::from_str(from).unwrap()),
                            ScVal::Address(ScAddress::from_str(to).unwrap()),
                            ScVal::Address(ScAddress::from_str(spender).unwrap()),
                            ScVal::Vec(Some(requests_to_scvec(requests))),
                        ])
                        .unwrap(),
                    },
                ),
                auth: VecM::default(),
            }),
        }
    }
    pub fn bad_debt(&self, user: &str) -> Operation {
        Operation {
            source_account: None,
            body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                    InvokeContractArgs {
                        contract_address: ScAddress::Contract(self.contract_id.clone()),
                        function_name: ScSymbol::try_from("bad_debt").unwrap(),
                        args: VecM::try_from(vec![ScVal::Address(
                            ScAddress::from_str(user).unwrap(),
                        )])
                        .unwrap(),
                    },
                ),
                auth: VecM::default(),
            }),
        }
    }

    pub fn new_bad_debt_auction(&self) -> Operation {
        Operation {
            source_account: None,
            body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                    InvokeContractArgs {
                        contract_address: ScAddress::Contract(self.contract_id.clone()),
                        function_name: ScSymbol::try_from("new_bad_debt_auction").unwrap(),
                        args: VecM::try_from(vec![]).unwrap(),
                    },
                ),
                auth: VecM::default(),
            }),
        }
    }
    pub fn new_liquidation_auction(&self, user: &str, percent_liquidated: u64) -> Operation {
        Operation {
            source_account: None,
            body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                    InvokeContractArgs {
                        contract_address: ScAddress::Contract(self.contract_id.clone()),
                        function_name: ScSymbol::try_from("new_liquidation_auction").unwrap(),
                        args: VecM::try_from(vec![
                            ScVal::Address(ScAddress::from_str(user).unwrap()),
                            ScVal::U64(percent_liquidated),
                        ])
                        .unwrap(),
                    },
                ),
                auth: VecM::default(),
            }),
        }
    }
    pub fn get_last_price(&self, asset: &Hash) -> Operation {
        Operation {
            source_account: None,
            body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                    InvokeContractArgs {
                        contract_address: ScAddress::Contract(self.contract_id.clone()),
                        function_name: ScSymbol::try_from("lastprice").unwrap(),
                        args: VecM::try_from(vec![ScVal::Vec(Some(
                            ScVec::try_from(vec![
                                ScVal::Symbol(ScSymbol::try_from("Stellar").unwrap()),
                                ScVal::Address(ScAddress::Contract(asset.clone())),
                            ])
                            .unwrap(),
                        ))])
                        .unwrap(),
                    },
                ),
                auth: VecM::default(),
            }),
        }
    }

    pub fn get_balance(&self, user: &str) -> Operation {
        let address = ScAddress::from_str(user).unwrap();
        Operation {
            source_account: None,
            body: stellar_xdr::curr::OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: stellar_xdr::curr::HostFunction::InvokeContract(
                    InvokeContractArgs {
                        contract_address: ScAddress::Contract(self.contract_id.clone()),
                        function_name: ScSymbol::try_from("balance").unwrap(),
                        args: VecM::try_from(vec![ScVal::Address(address)]).unwrap(),
                    },
                ),
                auth: VecM::default(),
            }),
        }
    }
}

fn requests_to_scvec(requests: Vec<Request>) -> ScVec {
    let mut vec = Vec::default();
    for request in requests.iter() {
        let address_val: ScVal =
            ScVal::Address(ScAddress::from_str(request.address.clone().as_str()).unwrap());
        let map = ScVal::Map(Some(ScMap(
            VecM::try_from(vec![
                ScMapEntry {
                    key: from_string_primitive("address", &ScSpecTypeDef::Symbol).unwrap(),
                    val: address_val,
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
