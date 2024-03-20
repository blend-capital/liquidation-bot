pub const STELLAR_POOL: &str = "CB6S4WFBMOJWF7ALFTNO3JJ2FUJGWYXQF3KLAN5MXZIHHCCAU23CZQPN";
pub const BRIDGE_POOL: &str = "CDGCNXWGZKZB5ZF7CVEVLDQ6YEP6QCRLZB32NMKHEUUEJIMVNTAAERD2";
pub const USDC: &str = "CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU";
pub const XLM: &str = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC";
pub const BTC: &str = "CAP5AMC2OHNVREO66DFIN6DHJMPOBAJ2KCDDIMFBR7WWJH5RZBFM3UEI";
pub const ETH: &str = "CAZAQB3D7KSLSNOSQKYD2V4JP5V2Y3B4RDJZRLBFCCIXDCTE3WHSY3UE";
pub const BACKSTOP: &str = "CAYRY4MZ42MAT3VLTXCILUG7RUAPELZDCDSI2BWBYUJWIDDWW3HQV5LU";
pub const BACKSTOP_TOKEN: &str = "CBESO2HJRRXRNEDNZ6PAF5FXCLQNUSJK6YRWWY2CXCIANIHTMQUTHSOM";
pub const ORACLE: &str = "CDLT57WKQHCIYVODTN7KGTU3RKXDHZK3EPVQB2QIGYWOBVEYEELFVVZO";
pub const STELLAR_POOL_LOCAL: &str = "CB7G3UYLVPNNQWEI35CFZOC3XFFXXRZUKSOEFKMFBD7CUYBNK277JMZO";
pub const BRIDGE_POOL_LOCAL: &str = "CCQE5NL352DTCMNLSQATY2BEQ5CCKO7EU6VKWTPWXVHCETEWC2JVKK4H";
pub const USDC_LOCAL: &str = "CAPCGZLDC4GWUXZV3XDWCJ2E2PTSAJGQX447A664R4ZLKZNBZZHHEKIF";
pub const XLM_LOCAL: &str = "CDMLFMKMMD7MWZP3FKUBZPVHTUEDLSX4BYGYKH4GCESXYHS3IHQ4EIG4";
pub const BTC_LOCAL: &str = "CDSCLL7BJNHYKWOQCKTI3JAQ4TGCSJD2GIQKCPW6NS76DNVIIYNCREVZ";
pub const ETH_LOCAL: &str = "CDNGTEWJCOPWE7OVYMH5M4ERUNOGDWXJXRQWMERBQOZGBDIWENHMT522";
pub const BACKSTOP_LOCAL: &str = "CA5RMDOVVDRBPFHVLQ64NZP6WWBZFWOOIJ4SXQDADZ2PFEGAQKAJUSRU";
pub const BACKSTOP_TOKEN_LOCAL: &str = "CAG7CBWI6WOMU7FWVTSERVTXFP43R52DYEWMZYS63HRB24JVFBPJMPXM";
pub const ORACLE_LOCAL: &str = "CCDUCIYTKYYPIVKF7HWI2OV2S2TNUBWPJA2H2OYYXXV7O64KPOAZWAPV";

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
