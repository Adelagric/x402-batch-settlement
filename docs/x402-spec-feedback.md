# x402 spec — feedback from a Rust implementation

Two precise discrepancies found while implementing x402 V2 from the
spec, each verified by reproducing `viem`'s output byte-for-byte.

## 1. `voucher.ts` docstring misstates `channelId` derivation

- **Where**: `typescript/packages/mechanisms/evm/src/batch-settlement/client/voucher.ts`, parameter doc for `channelId`.
- **Says**: `channelId` is `keccak256(abi.encode(ChannelConfig))`.
- **Actual** (`.../batch-settlement/utils.ts::computeChannelId`):
  `hashTypedData({ domain, types: channelConfigTypes, primaryType:
  "ChannelConfig", message })` — a full EIP-712 hash
  (`keccak256(0x1901 ‖ domainSeparator ‖ hashStruct)`), bound to
  `chainId` and the `x402BatchSettlement` verifying contract.
- **Impact**: an implementer following the docstring derives a wrong
  `channelId`; all vouchers then fail verification with no obvious
  cause.
- **Suggested fix**: correct the docstring to reference the EIP-712
  typed-data hash (and point to `computeChannelId`).

## 2. `extra.assetTransferMethod` presence is inconsistent across the spec

- **Where**: `specs/schemes/exact/scheme_exact_evm.md` examples carry
  `extra.assetTransferMethod`; `specs/transports-v2/http.md`
  `PaymentRequired` example carries `extra` = `{name, version}` only.
- **Impact**: an implementer modeling `extra.assetTransferMethod` as
  required (reasonable from the `exact`/EVM examples) fails to
  deserialize the canonical transport `PaymentRequired`, i.e. fails
  on real 402 challenges.
- **Suggested fix**: state explicitly that `extra.assetTransferMethod`
  is optional in `PaymentRequired`, and align the examples.

Both were resolved in the Rust implementation by treating the EIP-712
hash as authoritative and `extra.assetTransferMethod` as optional;
golden tests decode the verbatim spec Base64 vectors and match
`channelId`, the `Voucher` digest, and recovered signer exactly.
