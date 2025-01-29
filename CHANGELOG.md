# Changelog

## Unreleased

### `malachitebft-core-consensus`

#### Breaking Changes

- `ValuePayload` enum moved from `malachitebft_core_consensus` to `malachitebft_core_types`
- Added new variants to `Effect` enum:
  - `Effect:ExtendVote`
- Renamed variants from `Effect` enum:
  - `Effect::PersistMessage` to `Effect::WalAppendMessage`
  - `Effect::PersistTimeout` to `Effect::WalAppendTimeout`
- Removed struct `ValueToPropose` from `informalsystems_malachitebft_core_consensus`
  - Use `LocallyProposedValue` instead
- Removed field `extension` from struct `ProposedValue`

### `malachitebft-app`

#### Breaking Changes

- Merged `host` module into `types`

### `malachitebft-app-channel`

#### Breaking Changes

- Added `PeerJoined` and `PeerLeft` variants to `AppMsg` enum

### `malachitebft-engine`

#### Breaking Changes

- Added `PeerJoined ` and `PeerLeft` variants to `HostMsg` enum

## v0.0.1

Initial (pre-alpha) release.
