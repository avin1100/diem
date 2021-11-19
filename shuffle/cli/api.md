# Shuffle API

## shuffle new

**Usage:**

`shuffle new ~/Desktop/TestCoin`

Will take in a project_path (~/Desktop/TestCoin) and create a new shuffle project at that location with all pre-generated folders which can be used down the line. Also creates ~/.shuffle/Networks.toml which is your networks configuration file.

Note: Feel free to add any network you’d like to connect to with the same format as the localhost network (already in the file) to Networks.toml.

## shuffle node

**Usage:**

`shuffle node`

Runs a local node at endpoint http://127.0.0.1:8080 which will be used to listen to all transactions that is sent by the user. Also creates ~/.shuffle/nodeconfig directory which contains information to configure the local node.

`shuffle node --genesis diem-move/diem-framework/experimental`

Runs a local node with specific move package as the genesis modules (diem-move/diem-framework/experimental)

## shuffle account

**Usage:**

`shuffle account`

Creates an account on localhost by claiming an address. Also saves account information locally in your ~/.shuffle/networks/localhost/accounts.

`shuffle account --root ~/.shuffle/nodeconfig/mint.key`

Creates an account from root/mint.key path passed in by user (~/.shuffle/nodeconfig/mint.key)

`shuffle account --network trove_testnet`

Creates an account onto specified network. Saves account information locally in your ~/.shuffle/networks/[network_name]/accounts.
Note: the network name that is passed in must exist in the Networks.toml file.

## shuffle deploy

**Usage:**

`shuffle deploy --package ~/Desktop/TestCoin`

Publishes the main move package in your project folder (~/Desktop/TestCoin) to localhost using the account as publisher

`shuffle deploy --package ~/Desktop/TestCoin --network trove_testnet`

Publishes the main move package onto specified network (trove_testnet).
Note: the network name that is passed in must exist in the Networks.toml file.

## shuffle console

**Usage:**

`shuffle console --package ~/Desktop/TestCoin`

Enters typescript REPL for onchain inspection (on network localhost) of deployed project (~/Desktop/TestCoin).

`shuffle console --package ~/Desktop/TestCoin --network trove_testnet`

Enters REPL for onchain inspection on specified network (trove_testnet).
Note: the network name that is passed in must exist in the Networks.toml file.

`shuffle console --package ~/Desktop/TestCoin --key-path ~/.shuffle/networks/localhost/accounts/latest/dev.key --address 0x24163AFCC6E33B0A9473852E18327FA9`

Enters repl for inspection on certain key_path and address. Note: when using the key_path and address flags, they must both be passed in.

## shuffle build

**Usage:**

`shuffle build --package ~/Desktop/TestCoin`

Compiles the move package in your project folder (~/Desktop/TestCoin) and generates typescript files

## shuffle test

**Usage:**

`shuffle test e2e --package ~/Desktop/TestCoin`

Runs end-to-end test in shuffle with project folder (~/Desktop/TestCoin) on localhost

`shuffle test e2e --package ~/Desktop/TestCoin --network localhost`

Runs end-to-end test in shuffle with project folder (~/Desktop/TestCoin) on specific network (localhost)
Note: the network name that is passed in must exist in the Networks.toml file.

`shuffle test unit --package ~/Desktop/TestCoin`

Runs move unit tests in project folder

`shuffle test all --package ~/Desktop/TestCoin --network trove_testnet`

Runs both move unit tests in project folder and end-to-end test in shuffle with project folder (~/Desktop/TestCoin) on specific network (trove_testnet)
Note: the network name that is passed in must exist in the Networks.toml file.

## shuffle transactions

**Usage:**

`shuffle transactions`

Captures last 10 transactions from the account on the localhost network in pretty formatting

`shuffle transactions --raw`

Captures last 10 transactions from the account on the localhost network without pretty formatting

`shuffle transactions --tail`

Captures last 10 transactions from the account on the localhost network in pretty formatting and blocks/continuously polls for incoming transactions

`shuffle transactions --network localhost`

Captures the last 10 transactions from a given network (localhost).
Note: the network name that is passed in must exist in the Networks.toml file.

`shuffle transactions --address 24163AFCC6E33B0A9473852E18327FA9`

Captures the last 10 transactions deployed by a given address (24163AFCC6E33B0A9473852E18327FA9)

**These flags can be used together in a number of ways:**

`shuffle transactions --network trove_testnet --address 0x0000000000000000000000000B1E55ED --tail --raw`

Captures the last 10 transactions of address 0xB1E55ED on network trove_testnet without pretty formatting and also blocks and continuously polls for incoming transactions
