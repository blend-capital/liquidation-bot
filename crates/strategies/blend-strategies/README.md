# Blend Liquidation & Auction Strategies

This folder defines 2 strategies that are used to liquidate blend users. At a high level, the strategies monitor user's with non-insignificant leverage ratios as well as oracle prices. They create liquidation auctions for user's when their borrow limit is exceeded, then monitors and attempts to fill the aucitons whenever it's profitable. You don't need to run both of these strategies, but if you elect to run just the liquidator strategy, an additional process needs to be run to update the asset price and data db's as it relies on the auctioneer doing so.

## Blend Auctioneer Strategy

This strategy is responsible for monitoring blend users and oracle prices, and creating liquidation auctions for users when their borrow limit is exceeded.

### Sync

The strategy first syncs its initial state, by storing pool data, asset prices, and user positions in memory. We do this as follows:

1. Pull the price of all input assets from the oracle contract and store them in the `blend_assets` sql
2. Pull the reserve configurations of all input pools and assets and store them in the `blend_assets` db
3. Evaluate all user stored in the `blend_users` db and store their positions in memory if they have significant leverage levels.

### Processing

After the initial sync is done, we stream the following events:

1. New Blocks: Every 10 new blocks we update asset prices and check all user's we're tracking to see if they need to be liquidated. If they do, we create a liquidation auction for them.
2. Pool Events: we stream events from the blend pools and update the pool data and user positions in memory. We also add new user's to the `blend_users` db and add user positions to memory if they reach a significant leverage level.

## Blend Liquidator Strategy

This strategy is responsible for monitoring blend liquidation auctions and filling them whenever it's profitable.

### Sync

The strategy first syncs its initial state, by storing the liquidators assets and positions and storing ongoing liquidations in memory. We do this as follows:

1. Pull all liquidator asset balances and store them in memory.
2. Pull liquidator positions for all pools we're liquidating in and store them in memory.
3. Pull all ongoing interest auctions and store them in memory.
4. Pull all ongoing bad debt auctions and store them in memory.
5. Pull all ongoing liquidation auctions and store them in memory.

### Processing

After the initial sync is done, we stream the following events:

1. New Blocks: Whenever we get a new block we check if any of our tracked liquidation auctions can be profitably filled. If they can, we fill them.
2. Pool Events: We stream events to pick up any new liquidations auctions, and remove one's that we're tracking that have been filled.

## Contracts

These strategies does not rely on any contracts.

## Build and Test

You can run the rust tests with the following command:

```sh
cargo test
```

And if you need to regenerate rust bindings for contracts, you can run
