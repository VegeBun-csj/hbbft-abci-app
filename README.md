# hbbft-abci-tendermint
This is a practice of replacing the tendermint core with HoneyBadger BFT adapted to ABCI.

Our implementation process roughly consists of the following two steps:

* Implementing the complete invocation of the hbbft consensus algorithm, using the Rust library developed for [hbbft](https://github.com/poanetwork/hbbft) in POA. We will showcase the protocol process through the form of nodes and perform testing.
* Combining the implemented complete process with the ABCI interface, replacing the Tendermint consensus, and modularizing it into an ABCI client to adapt to the development of other apps (ABCI servers).