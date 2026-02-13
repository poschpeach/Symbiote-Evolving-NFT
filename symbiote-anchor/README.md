# Symbiote Anchor Program

## Build

```bash
# from root/symbiote-anchor
cd symbiote-anchor
anchor build
```

## Test (local validator)

```bash
# from root/symbiote-anchor
cd symbiote-anchor
yarn
anchor test
```

## Deploy (dev pipeline)

```bash
# from root/symbiote-anchor
cd symbiote-anchor
./scripts/deploy-dev.sh
```

## Program Instructions

- `mint_symbiote(owner: Pubkey)`
- `evolve_symbiote(nft_account: Pubkey, new_stats: Stats)`

## Metadata URI

Current on-chain URI base is set for local dev:

- `http://localhost:3000/metadata/<mint>/state.json?...`

Before mainnet deployment, update URI base in program source and redeploy.
