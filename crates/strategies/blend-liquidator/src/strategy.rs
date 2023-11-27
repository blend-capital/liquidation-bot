use std::collections::HashMap;

use std::future::Pending;
use std::sync::Arc;
use std::str::FromStr;

use async_trait::async_trait;

use ethers::contract::multicall_contract;
use stellar_xdr::next::{ContractEvent,ContractEventBody, ScAddress, VecM, Hash, ContractEventV0, ScSymbol, StringM, WriteXdr};
use tracing::info;

use crate::constants::FACTORY_DEPLOYMENT_BLOCK;
use crate::types::{Config, UserPositions, ReserveConfig};
use anyhow::Result;
use artemis_core::collectors::block_collector::NewBlock;
use artemis_core::collectors::opensea_order_collector::OpenseaOrder;
use artemis_core::executors::soroban_executor::{GasBidInfo, SubmitStellarTx};
use artemis_core::types::Strategy;
use artemis_core::utilities::state_override_middleware::StateOverrideMiddleware;
use ethers::providers::Middleware;
use ethers::types::{Filter, H256};
use ethers::types::{H160, U256};
use opensea_stream::schema::Chain;
use opensea_v2::client::OpenSeaV2Client;

use super::types::{
    Action,
    Event,PendingFill
};

#[derive(Debug, Clone)]
pub struct BlendLiquidator {
    /// Ethers client.
    // client: Arc<M>,
    /// Opensea V2 client
    // opensea_client: OpenSeaV2Client,
    /// LSSVM pair factory contract for getting pair history.
    // lssvm_pair_factory: Arc<LSSVMPairFactory<M>>,
    /// Quoter for batch reading pair state.
    // quoter: SudoPairQuoter<StateOverrideMiddleware<Arc<M>>>,
    /// Assets we're interested in
    assets: Vec<Hash>,
    /// Map Assets to bid on and their prices
    asset_prices: HashMap<Hash, i128>,
    /// Vec of Blend pool addresses to bid on auctions in
    pools: Vec<Hash>,
    /// Oracle ID for getting asset prices
    oracle_id: Hash,
    /// Amount of profits to bid in gas
    bid_percentage: u64,
    /// Pending auction fills
    pending_fill: Vec<PendingFill>,
    /// Map of users and their positions in pools
    users: HashMap<Hash, HashMap<Hash,UserPositions>>,
    /// Map of pools and their reserve configurations
    reserve_configs: HashMap<Hash, HashMap<Hash,ReserveConfig>>,
    /// Our positions
    bankroll: HashMap<Hash,UserPositions>,
    /// Our address
    us: Hash,
    // Our minimum health factor
    min_hf: i128,
    
}

impl BlendLiquidator {
    pub fn new(config: Config) -> Self {
        // // Set up LSSVM pair factory contract.
        // let lssvm_pair_factory = Arc::new(LSSVMPairFactory::new(
        //     *LSSVM_PAIR_FACTORY_ADDRESS,
        //     client.clone(),
        // ));
        // // Set up Sudo pair quoter contract.
        // let mut state_override = StateOverrideMiddleware::new(client.clone());
        // // Override account with contract bytecode
        // let addr = state_override.add_code(SUDOPAIRQUOTER_DEPLOYED_BYTECODE.clone());
        // // Instantiate contract with override client
        // let quoter = SudoPairQuoter::new(addr, Arc::new(state_override));
        // // Set up arb contract.
        // let arb_contract = SudoOpenseaArb::new(config.arb_contract_address, client.clone());

        Self {
            // client,
            // opensea_client,
            // lssvm_pair_factory,
            // quoter,
            // arb_contract,
            assets : config.assets,
            asset_prices: HashMap::new(),
            pools: config.pools,
            oracle_id: config.oracle_id,
            bid_percentage: config.bid_percentage,
            pending_fill: vec![],
            users: HashMap::new(),
            reserve_configs: HashMap::new(),
            bankroll: HashMap::new(),
            us: config.us,
            min_hf: config.min_hf,
        }
    }
}

#[async_trait]
impl Strategy<Event, Action> for BlendLiquidator {
    // In order to sync this strategy, we need to get the current bid for all Sudo pools.
    async fn sync_state(&mut self) -> Result<()> {
        // // Block in which the pool factory was deployed.
        // let start_block = FACTORY_DEPLOYMENT_BLOCK;

        // let current_block = self.client.get_block_number().await?.as_u64();

        // Get all asset prices
        self.get_asset_prices(self.assets.clone()).await?;

        // Get user positions in given pools - also fill in our positions

        // Get reserve configs for given pools

        // info!(
        //     "done syncing state, found available pools for {} collections",
        //     self.sudo_pools.len()
        // );

        Ok(())
    }

    // Process incoming events, filter non-auction events, decide if we care about auctions
    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        //
        let mut actions: Vec<Action> = [].to_vec();
        match event {
            Event::SorobanEvents(events) => {
                let events = *events;
                println!("received {} soroban events ", events.len());
                
            },
            Event::NewBlock(block) => {
                //TODO decide whether we need to execute a pending auction
            }
            // self
            //     .process_soroban_events(events)
            //     .await
            //     .map_or(vec![], |a| vec![a]),
        }
        actions
    }
}

impl BlendLiquidator {
    // Process new orders as they come in.
    async fn process_soroban_events(&mut self, events: VecM<ContractEvent>) -> Option<Action> {
        //should build pending auctions and remove or modify pending auctions that are filled or partially filled by someone else
        for event in events.iter() {
            let contract_id = event.contract_id.clone().unwrap();
            if self.oracle_id != contract_id && !self.pools.contains(&(&contract_id)) {
                continue;
            }
            // ContractEventBody::V0(event);
            // let body_v0 = event.body();
            let body = event.body.clone();
      
            match body {
                ContractEventBody::V0(v0) => {
                    let name = v0.topics.get(0).unwrap();
                    let data = v0.data.clone();

                    let name_xdr = name.to_xdr().unwrap();
                    if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("new_liquidation_auction").unwrap())).to_xdr().unwrap()) {
                        let collateral: HashMap<Hash, i128> = HashMap::new(); //TODO grab from event
                        let liabilities: HashMap<Hash, i128> = HashMap::new(); //TODO grab from event
                        let user: Hash = Hash([0; 32]); //TODO grab from event 
                        self.create_pending_fill(contract_id.clone(), user, collateral, liabilities, false)
                        // Decide whether to fill this auction based on if we're comfortable with the assets being auctioned off
                        // Decide when to fill this auction based on our required profitability
                        // Decide how much of this auction to fill based on the amount of assets we can support given pool positions
                        // If yes add to a list of pending auctions (pct filled 0%)
                       
                    } else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("delete_liquidation_auction").unwrap())).to_xdr().unwrap()) {
                        // If this was an auction we were planning on filling, remove it from the pending list
                    } else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("new_auction").unwrap())).to_xdr().unwrap()) {
                        // If a bad debt auction
                            // Decide whether to fill this auction based on if we're comfortable with the assets being auctioned off
                            // Decide when to fill this auction based on our required profitability
                            // Decide how much of this auction to fill based on the amount of assets we can support given pool positions
                            // If yes add to a list of pending auctions (pct filled 0%)
                        // If an interest auction
                            // Decide whether to fill this auction based on whether we're comfortable with the assets being auctioned off
                            // Decide when to fill this auction based on our required profitability
                            // Decide how much of this auction to fill based on how much USDC we have/can source - we will attempt to borrow the necessary USDC from the pool holding the auction (this just keeps the bot simple)
                    } else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("fill_auction").unwrap())).to_xdr().unwrap()) {
                        // Check if we were planning on filling the auction
                        // If yes, if the fill percent was 100 then remove it from the pending list
                        // If the fill percent was less than 100 then update the pending list with the new fill percent (consider the last fill percent, new fill pct = old_pct+((1-old_pct)*new_pct) )
                        // also update the percent that we were planning on filling 
                        // Note: we gotta differentiate btw backstop and interest auctions here
                        // we should check whether bad debt was created if the auction was filled post 200 blocks and if so attempt to move bad debt to the backstop and see if we can liquidate it - may be uneccesary
                     } else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("update_reserve").unwrap())).to_xdr().unwrap()) {
                        // Update the reserve config for the pool
                    }else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("supply").unwrap())).to_xdr().unwrap()) {
                        // Update reserve estimated b rate by using request.amount/b_tokens_minted from the emitted event
                    }else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("withdraw").unwrap())).to_xdr().unwrap()) {
                        // Update reserve estimated b rate by using tokens out/b tokens burned from the emitted event
                    }else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("supply_collateral").unwrap())).to_xdr().unwrap()) {
                        // Update reserve estimated b rate by using request.amount/b_tokens_minted from the emitted event
                        // Update users collateral positions
                    }else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("withdraw_collateral").unwrap())).to_xdr().unwrap()) {
                        // Update reserve estimated b rate by using tokens out/b tokens burned from the emitted event
                        // Update users collateral positions
                    }else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("borrow").unwrap())).to_xdr().unwrap()) {
                        // Update reserve estimated d rate by using request.amount/d tokens minted from the emitted event
                        // Update users liability positions
                    }else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("repay").unwrap())).to_xdr().unwrap()) {
                        // Update reserve estimated d rate by using request.amount/d tokens burnt from the emitted event
                        // Update users liability positions
                    }else if name_xdr.eq(&stellar_xdr::next::ScVal::Symbol(ScSymbol::from(StringM::from_str("oracle_update").unwrap())).to_xdr().unwrap()) {
                        // Update the asset price
                        // Check if we can liquidate anyone based on the new price
                        // If we can, create an auction
                        // We probably need to re-assess auction profitability here
                    }

                         
                    else {
                        //No action needed 
                        println!("unknown event");
                    }

                }
                _ => {
                    //TODO
                    println!("unknown event");
                }
            }            

        }
         
        None
        // Build arb tx.
        // self.build_arb_tx(event.listing.order_hash, *max_pool, *max_bid)
        //     .await
        
    }

    /// Process new block events, updating the internal state.
    fn process_new_block_event(&mut self, event: NewBlock,actions: &mut Vec<Action>) -> Option<Vec<Action>> {
        for pending in self.pending_fill.iter_mut(){   
            if pending.target_block <= event.number.as_u64() {
                // Create a fill tx
            }
        }
        None
    }

    async fn get_asset_prices(&mut self, assets: Vec<Hash>) -> Result<()> {
        // get asset prices from oracle
        let oracle_prices: Vec<i128> = vec![]; //TODO
        let res = assets
            .into_iter()
            .zip(oracle_prices)
            .collect::<HashMap<Hash, i128>>();
        self.asset_prices = res.clone();
        Ok(())
    }

    async fn get_reserve_config(&mut self, assets: Vec<Hash>) {
        //TODO
        // we need to use a client to grab pool data and run through it to get reserve config for assets 

    }

    async fn update_user_positions(&mut self, pool: Hash) {
        //TODO
        // we need to use the client to grab all users and store their positions - also needs to recognize us and store our position data in the bankroll
    }

     fn check_health(&self, user: Hash) -> bool {
        //TODO
        // Check user health return true if healthy
        true
    }

    fn create_pending_fill(&mut self, pool: Hash, user: Hash, collateral: HashMap<Hash, i128>, liabilities: HashMap<Hash, i128>, interest_auction: bool) {
        //TODO
        // Create a pending fill and add it to the pending fill list
    }

    /// Build arb tx from order hash and sudo pool params.
    async fn build_create_auction_tx(
        &self,
        order_hash: H256,
        sudo_pool: H160,
        sudo_bid: U256,
    ) -> Option<Action> {
        // Get full order from Opensea V2 API.
        let response = self
            .opensea_client
            .fulfill_listing(hash_to_fulfill_listing_request(order_hash))
            .await;
        let order = match response {
            Ok(order) => order,
            Err(e) => {
                info!("Error getting order from opensea: {}", e);
                return None;
            }
        };

        // Parse out arb contract parameters.
        let payment_value = order.fulfillment_data.transaction.value;
        let total_profit = sudo_bid - payment_value;

        // Build arb tx.
        let tx = self
            .arb_contract
            .execute_arb(
                fulfill_listing_response_to_basic_order_parameters(order),
                payment_value.into(),
                sudo_pool,
            )
            .tx;
        Some(Action::SubmitTx(SubmitTxToMempool {
            tx,
            gas_bid_info: Some(GasBidInfo {
                total_profit,
                bid_percentage: self.bid_percentage,
            }),
        }))
    }

}
