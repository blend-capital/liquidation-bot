# Blend Inventory Management

## Dependancies:

- A parallel strategy must update Blend pool reserve configs and b and d token rates

## Strategy

### Sync

The strategy first syncs its initial state, by getting our positions in relevant blend pools and pulling in spot asset prices. We do this as follows:

1. For all input pools pull our positions
2. Pull our wallet balances of all input assets
3. Pull coinbase bid_ask for all pairs we care about

### Processing

After the initial sync is done, we stream the following events:

1. New Blocks: for every new block we check if our orders were touched and repay with collected proceeds. If we still have debt pull more collateral to the min_hf and attempt to sell it.
2. Soroban Events: Updates our positions when we fill a new liquidation auction.
3. Seaport orders: we stream seaport orders, filtering for sell orders on the collections which have valid sudo quotes. We compute whether an arb is available, and if so, submit a transaction to our atomic arb contract.

## Contracts

This strategy relies on two contracts:

1. [`SudoOpenseaArb`](/crates/strategies/opensea-sudo-arb/contracts/src/SudoOpenseaArb.sol): Execute an atomic arb by buying an NFT on seaport by calling `fulfillBasicOrder`, and selling it on Sudoswap by calling `swapNFTsForToken`.

2. [`SudoPairQuoter`](/crates/strategies/opensea-sudo-arb/contracts/src/SudoPairQuoter.sol): Batch read contract that checks whether sudo pools have valid quotes.

## Build and Test

In order to run the solidity test, you need access to an alchemy/infura key. You can run tests with the following command:

```sh
ETH_MAINNET_HTTP=<YOUR_RPC_URL> forge test --root ./contracts
```

You can run the rust tests with the following command:

```sh
cargo test
```

And if you need to regenerate rust bindings for contracts, you can run

```sh
forge bind --bindings-path ./bindings --root ./contracts --crate-name bindings --overwrite
```
