pub const STELLAR_POOL: &str = "CB6S4WFBMOJWF7ALFTNO3JJ2FUJGWYXQF3KLAN5MXZIHHCCAU23CZQPN";
pub const BRIDGE_POOL: &str = "CDGCNXWGZKZB5ZF7CVEVLDQ6YEP6QCRLZB32NMKHEUUEJIMVNTAAERD2";
pub const USDC: &str = "CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU";
pub const XLM: &str = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC";
pub const BTC: &str = "CAP5AMC2OHNVREO66DFIN6DHJMPOBAJ2KCDDIMFBR7WWJH5RZBFM3UEI";
pub const ETH: &str = "CAZAQB3D7KSLSNOSQKYD2V4JP5V2Y3B4RDJZRLBFCCIXDCTE3WHSY3UE";
pub const BACKSTOP: &str = "CAYRY4MZ42MAT3VLTXCILUG7RUAPELZDCDSI2BWBYUJWIDDWW3HQV5LU";
pub const BACKSTOP_TOKEN: &str = "CBESO2HJRRXRNEDNZ6PAF5FXCLQNUSJK6YRWWY2CXCIANIHTMQUTHSOM";
pub const ORACLE: &str = "CDLT57WKQHCIYVODTN7KGTU3RKXDHZK3EPVQB2QIGYWOBVEYEELFVVZO";
pub const STELLAR_POOL_LOCAL: &str = "CAXQUMNIB4EHLDLZEJBH2GABZWONLL2BO6FGNHFJL74CWPTXJ5WEGTAS";
pub const BRIDGE_POOL_LOCAL: &str = "CBZ4YY4L6MTTRYNHILKFERFJQXNRYQ2FJQUYMVBKPSJ3HEI37BWKOV5S";
pub const USDC_LOCAL: &str = "CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU";
pub const XLM_LOCAL: &str = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC";
pub const BTC_LOCAL: &str = "CAP5AMC2OHNVREO66DFIN6DHJMPOBAJ2KCDDIMFBR7WWJH5RZBFM3UEI";
pub const ETH_LOCAL: &str = "CAZAQB3D7KSLSNOSQKYD2V4JP5V2Y3B4RDJZRLBFCCIXDCTE3WHSY3UE";
pub const BACKSTOP_LOCAL: &str = "CBF74HTJOSBUPSCKJYVM6AEIR55LE4INPYQPKM2NJ3YEV4LB25AVJQ6D";
pub const BACKSTOP_TOKEN_LOCAL: &str = "CBPK2X2RUEYMWLSLTOJ4EFMYK4LF3XBPU7BMGNNKGZR7AE3RG7YQDMCV";
pub const ORACLE_LOCAL: &str = "CBCUV6SXJEOAH3BF7V2AA5AEQRDQ5IPXYDWUGTUJET4ZJUSHGEHDKOPL";

pub struct ContractAddress;

impl ContractAddress {
    pub fn get_stellar_pool(input: i32) -> &'static str {
        match input {
            0 => STELLAR_POOL_LOCAL,
            1 => STELLAR_POOL,
            _ => panic!("Invalid input"),
        }
    }

    pub fn get_bridge_pool(input: i32) -> &'static str {
        match input {
            0 => BRIDGE_POOL_LOCAL,
            1 => BRIDGE_POOL,
            _ => panic!("Invalid input"),
        }
    }

    pub fn get_usdc(input: i32) -> &'static str {
        match input {
            0 => USDC_LOCAL,
            1 => USDC,
            _ => panic!("Invalid input"),
        }
    }

    pub fn get_xlm(input: i32) -> &'static str {
        match input {
            0 => XLM_LOCAL,
            1 => XLM,
            _ => panic!("Invalid input"),
        }
    }

    pub fn get_btc(input: i32) -> &'static str {
        match input {
            0 => BTC_LOCAL,
            1 => BTC,
            _ => panic!("Invalid input"),
        }
    }

    pub fn get_eth(input: i32) -> &'static str {
        match input {
            0 => ETH_LOCAL,
            1 => ETH,
            _ => panic!("Invalid input"),
        }
    }

    pub fn get_backstop(input: i32) -> &'static str {
        match input {
            0 => BACKSTOP_LOCAL,
            1 => BACKSTOP,
            _ => panic!("Invalid input"),
        }
    }

    pub fn get_backstop_token(input: i32) -> &'static str {
        match input {
            0 => BACKSTOP_TOKEN_LOCAL,
            1 => BACKSTOP_TOKEN,
            _ => panic!("Invalid input"),
        }
    }

    pub fn get_oracle(input: i32) -> &'static str {
        match input {
            0 => ORACLE_LOCAL,
            1 => ORACLE,
            _ => panic!("Invalid input"),
        }
    }
}
