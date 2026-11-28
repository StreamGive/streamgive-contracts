# Documentation

Guides for the `donation-vault` and `ngo-registry` contracts. For setup,
testing, deployment, pausing, and error codes, see the
[main README](../README.md).

| Document | What it covers |
| --- | --- |
| [DONORS.md](./DONORS.md) | A plain-language guide for donors: creating, topping up, pausing, and cancelling a stream, the fees involved, and what can go wrong. |
| [NGOS.md](./NGOS.md) | A plain-language guide for NGOs: registering, getting and keeping verified, receiving and withdrawing funds, setting a payout address, and what changes if verification is revoked. |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | How the contracts, backend, and frontend fit together, how a donation flows end to end, and which tokens the vault supports. |
| [STREAM_LIFECYCLE.md](./STREAM_LIFECYCLE.md) | The life of a stream from creation to cancellation, with sequence diagrams for each fork and how to convert a monthly amount into a per-second rate. |
| [ROLES.md](./ROLES.md) | Every public entry point of both contracts and the role (admin, approver, donor, NGO, anyone) it requires. |
| [API.md](./API.md) | Public function reference: arguments, authorization, errors, and events for every entry point. |
| [EVENTS.md](./EVENTS.md) | Every event the contracts publish, with its topics and data shape, for writing an indexer. |
| [INDEXING.md](./INDEXING.md) | Event ordering, replay/idempotency guidance, stream lifecycle, and an event-based state rebuild example. |
| [FEES.md](./FEES.md) | How the protocol fee is calculated, rounded, and paid to the treasury, with worked examples. |
| [STORAGE.md](./STORAGE.md) | Which storage type each `DataKey` uses, the TTL policy, and the keep-alive entry points that prevent archival. |
| [GLOSSARY.md](./GLOSSARY.md) | Precise definitions and examples for terms used in the stream lifecycle. |
| [CLI.md](./CLI.md) | Copy-ready Stellar CLI commands for common testnet operations. |
| [GAS.md](./GAS.md) | How to simulate each entry point with the `stellar` CLI and read its resource cost. |
| [THREAT_MODEL.md](./THREAT_MODEL.md) | What the contracts protect, who they defend against, what the admin can and cannot do, and which risks are accepted. |
| [UPGRADE_POLICY.md](./UPGRADE_POLICY.md) | What integrators can rely on between releases: append-only error codes and the rules for storage-layout changes. |
| [MAINNET_CHECKLIST.md](./MAINNET_CHECKLIST.md) | Pre-launch sign-offs for audit status, admin custody, fees, treasury, registry, pause readiness, monitoring, deployment, and post-deploy verification. |
| [adr/](./adr/README.md) | Architecture decision records: why the contracts are built the way they are (lazy accrual, checked arithmetic, the registry check, TTL policy), with a template for new records. |

When you add a document to this folder, add a row here in the same PR.
