## Blend Liquidation Bot

The Blend Liquidation Bot is a project that utilizes the [Artemis framework](https://github.com/paradigmxyz/artemis) to create a liquidation bot for the Blend protocol on the Soroban blockchain. The bot includes auction creation and filling functionalities that are triggered by contract events streamed from an rpc. The purpose of this bot is to create user liquidation, interest, and bad debt auctions and to fill them when the desired profitability is met. It is recommended to run a private rpc to submit transactions as public rpcs will lead to rate limiting issues.

## Running the Rust Project

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)

To run the project first clone the repository and navigate to the directory

```sh
git clone https://github.com/blend-capital/liquidation-bot
cd liquidation-bot
```

Then run the following command

```sh
cargo run -- --config-path "Path to config file" --secret-key "S...."
```

The config file contains the configuration parameters for the liquidator and auctioneer strategies. An example config file is located at the root called "example.config.json" Use this as a template and rename to config.json. An example config looks like

```json
{
  "rpc_url": "http://host.docker.internal:8000",
  "network_passphrase": "Test SDF Network ; September 2015",
  "db_path": "./",
  "pools": [
    "CB6S4WFBMOJWF7ALFTNO3JJ2FUJGWYXQF3KLAN5MXZIHHCCAU23CZQPN",
    "CB7SS5VTUQZGPDWPQKD4ZT4NSDX4BR5PJ55OE3GWZZYV3I5PAZBZ7CY5"
  ],
  "supported_collateral": [
    "CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU",
    "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
  ],
  "supported_liabilities": [
    "CAP5AMC2OHNVREO66DFIN6DHJMPOBAJ2KCDDIMFBR7WWJH5RZBFM3UEI",
    "CAZAQB3D7KSLSNOSQKYD2V4JP5V2Y3B4RDJZRLBFCCIXDCTE3WHSY3UE"
  ],
  "backstop": "CD66EGYOKJ4DPY4FADXZS5FNL3DEVANWRNPNVANF6RQIN44GDB3HKANF",
  "backstop_token_address": "CCDCG7SJSSBIDULL4HIMHH5SCAIATCACVD7JSEGZXORBYRNPV6B7LMLP",
  "usdc_token_address": "CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU",
  "bid_percentage": 0,
  "oracle_id": "CA2NWEPNC6BD5KELGJDVWWTXUE7ASDKTNQNL6DN3TGBVWFEWSVVGMUAF",
  "min_hf": 12000000,
  "required_profit": 10000000,
  "oracle_decimals": 7
}
```

The min_hf represents the minimum health factor of the liquidator in 9 decimals. The required_profit field is the desired profit on liquidations represented in 9 decimals.

The supported_collateral field represents the assets that the liquidator holds and will be used to cover the auction bid. The supported_liabilities represent the assets that the liquidator will receive from the lot. These controls allow the liquidator to choose what assets they interact with.

## Docker Image

### Building

To build the Docker image, navigate to the project directory (where the Dockerfile is located) and run the following command:

```sh
docker build -t blend-liquidation-bot .
```

This will build a Docker image named blend-liquidation-bot. You can run the image with the docker run command.

### Running

To run the docker image run the following command:

```sh
docker run --name blend-liquidation-bot --rm -it -v <"Absolute path to config folder">:/opt/liquidation-bot blend-liquidation-bot --private-key S....
```

The docker image will be run in interactive mode with the specified name. To run the image detached replace -it with -d

The -v flag will mount the local directory to the docker instance and persists data stored by the bot to the specified location. The config mentioned above must be located in this folder. The /opt/liquidation-bot should not be changed as this is the designated location inside the docker container where the project looks for the config. If the container is shutdown restarting the image with the same config directory will restore the bot's data.

Please replace blend-liquidation-bot and liq-bot with the actual name you want to use for your Docker image and instance name.

## Acknowledgements

- [Artemis](https://github.com/paradigmxyz/artemis)

## Disclaimer

Running this liquidation bot carries financial risk. The bot is not guaranteed to be profitable and may result in financial loss. The bot is provided as is and the developers are not responsible for any financial loss incurred by running the bot. It is recommended to run the bot on a testnet before running on mainnet.
