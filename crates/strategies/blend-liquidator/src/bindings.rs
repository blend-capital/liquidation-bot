pub const WASM: &[u8] = soroban_sdk::contractfile!(
    file = "../blend-contracts/target/wasm32-unknown-unknown/optimized/pool.wasm",
    sha256 = "77424ea55688e45c2e7697c78102b8ab0b610e45723cbec3b13d7288aa32e636"
); //TODO: should use protocols.json eventually
#[soroban_sdk::contractclient(name = "Client")]
pub trait Contract {
    fn initialize(
        env: soroban_sdk::Env,
        admin: soroban_sdk::Address,
        name: soroban_sdk::Symbol,
        oracle: soroban_sdk::Address,
        bstop_rate: u64,
        backstop_id: soroban_sdk::Address,
        blnd_id: soroban_sdk::Address,
        usdc_id: soroban_sdk::Address,
    );
    fn set_admin(env: soroban_sdk::Env, new_admin: soroban_sdk::Address);
    fn update_pool(env: soroban_sdk::Env, backstop_take_rate: u64);
    fn init_reserve(
        env: soroban_sdk::Env,
        asset: soroban_sdk::Address,
        config: ReserveConfig,
    ) -> u32;
    fn update_reserve(env: soroban_sdk::Env, asset: soroban_sdk::Address, config: ReserveConfig);
    fn get_positions(env: soroban_sdk::Env, address: soroban_sdk::Address) -> Positions;
    fn submit(
        env: soroban_sdk::Env,
        from: soroban_sdk::Address,
        spender: soroban_sdk::Address,
        to: soroban_sdk::Address,
        requests: soroban_sdk::Vec<Request>,
    ) -> Positions;
    fn bad_debt(env: soroban_sdk::Env, user: soroban_sdk::Address);
    fn update_status(env: soroban_sdk::Env) -> u32;
    fn set_status(env: soroban_sdk::Env, pool_status: u32);
    fn update_emissions(env: soroban_sdk::Env) -> u64;
    fn set_emissions_config(
        env: soroban_sdk::Env,
        res_emission_metadata: soroban_sdk::Vec<ReserveEmissionMetadata>,
    );
    fn claim(
        env: soroban_sdk::Env,
        from: soroban_sdk::Address,
        reserve_token_ids: soroban_sdk::Vec<u32>,
        to: soroban_sdk::Address,
    ) -> i128;
    fn new_liquidation_auction(
        env: soroban_sdk::Env,
        user: soroban_sdk::Address,
        percent_liquidated: u64,
    ) -> AuctionData;
    fn del_liquidation_auction(env: soroban_sdk::Env, user: soroban_sdk::Address);
    fn get_auction(
        env: soroban_sdk::Env,
        auction_type: u32,
        user: soroban_sdk::Address,
    ) -> AuctionData;
    fn new_auction(env: soroban_sdk::Env, auction_type: u32) -> AuctionData;
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AuctionData {
    pub bid: soroban_sdk::Map<soroban_sdk::Address, i128>,
    pub block: u32,
    pub lot: soroban_sdk::Map<soroban_sdk::Address, i128>,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReserveEmissionMetadata {
    pub res_index: u32,
    pub res_type: u32,
    pub share: u64,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Request {
    pub address: soroban_sdk::Address,
    pub amount: i128,
    pub request_type: u32,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Reserve {
    pub asset: soroban_sdk::Address,
    pub b_rate: i128,
    pub b_supply: i128,
    pub backstop_credit: i128,
    pub c_factor: u32,
    pub d_rate: i128,
    pub d_supply: i128,
    pub index: u32,
    pub ir_mod: i128,
    pub l_factor: u32,
    pub last_time: u64,
    pub max_util: u32,
    pub scalar: i128,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Positions {
    pub collateral: soroban_sdk::Map<u32, i128>,
    pub liabilities: soroban_sdk::Map<u32, i128>,
    pub supply: soroban_sdk::Map<u32, i128>,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct PoolConfig {
    pub bstop_rate: u64,
    pub oracle: soroban_sdk::Address,
    pub status: u32,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct PoolEmissionConfig {
    pub config: u128,
    pub last_time: u64,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReserveConfig {
    pub c_factor: u32,
    pub decimals: u32,
    pub index: u32,
    pub l_factor: u32,
    pub max_util: u32,
    pub r_one: u32,
    pub r_three: u32,
    pub r_two: u32,
    pub reactivity: u32,
    pub util: u32,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReserveData {
    pub b_rate: i128,
    pub b_supply: i128,
    pub backstop_credit: i128,
    pub d_rate: i128,
    pub d_supply: i128,
    pub ir_mod: i128,
    pub last_time: u64,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReserveEmissionsConfig {
    pub eps: u64,
    pub expiration: u64,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReserveEmissionsData {
    pub index: i128,
    pub last_time: u64,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct UserEmissionData {
    pub accrued: i128,
    pub index: i128,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct UserReserveKey {
    pub reserve_id: u32,
    pub user: soroban_sdk::Address,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AuctionKey {
    pub auct_type: u32,
    pub user: soroban_sdk::Address,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum PoolDataKey {
    ResConfig(soroban_sdk::Address),
    ResData(soroban_sdk::Address),
    EmisConfig(u32),
    EmisData(u32),
    Positions(soroban_sdk::Address),
    UserEmis(UserReserveKey),
    Auction(AuctionKey),
    AuctData(soroban_sdk::Address),
}
#[soroban_sdk::contracttype(export = false)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum Asset {
    Stellar(soroban_sdk::Address),
    Other(soroban_sdk::Symbol),
}
#[soroban_sdk::contracterror(export = false)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum PoolError {
    NotAuthorized = 1,
    BadRequest = 2,
    AlreadyInitialized = 3,
    NegativeAmount = 4,
    InvalidPoolInitArgs = 5,
    InvalidReserveMetadata = 6,
    InvalidHf = 10,
    InvalidPoolStatus = 11,
    InvalidUtilRate = 12,
    EmissionFailure = 20,
    StalePrice = 30,
    InvalidLiquidation = 100,
    InvalidLot = 101,
    InvalidBids = 102,
    AuctionInProgress = 103,
    InvalidAuctionType = 104,
    InvalidLiqTooLarge = 105,
    InvalidLiqTooSmall = 106,
    InterestTooSmall = 107,
}
