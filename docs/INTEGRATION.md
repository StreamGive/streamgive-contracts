# Integration guide for client developers

How to call `donation-vault` and `ngo-registry` from a TypeScript client
(frontend, backend, or indexer) using `@stellar/stellar-sdk`. This covers
the call order, auth, error handling, events, and retries — for storage
layout and breaking-change rules, see
[UPGRADE_POLICY.md](UPGRADE_POLICY.md); for event shapes, see
[EVENTS.md](EVENTS.md).

## 1. Simulate before you sign

Every write call (`create_stream`, `withdraw`, `approve_ngo`, ...) is a
contract invocation: build it, **simulate** it to get the resource fee and
the authorization entries it needs, then sign and submit. Never submit
without simulating first — the simulated resource footprint is what the
network charges for, and skipping it means guessing at fees and missing
step 2 below entirely.

```ts
import { Contract, rpc, TransactionBuilder, BASE_FEE } from '@stellar/stellar-sdk';

const server = new rpc.Server('https://soroban-testnet.stellar.org');
const contract = new Contract(donationVaultContractId);

const account = await server.getAccount(donorPublicKey);
const tx = new TransactionBuilder(account, {
  fee: BASE_FEE,
  networkPassphrase: 'Test SDF Network ; September 2015',
})
  .addOperation(
    contract.call(
      'create_stream',
      donorAddressScVal,
      ngoAddressScVal,
      tokenAddressScVal,
      depositScVal,
      rateScVal,
    ),
  )
  .setTimeout(30)
  .build();

const simulated = await server.simulateTransaction(tx);
if (rpc.Api.isSimulationError(simulated)) {
  // See "Reading errors" below before showing this to a user.
  throw new Error(simulated.error);
}

const prepared = rpc.assembleTransaction(tx, simulated).build();
```

## 2. Authorization entries

`simulateTransaction`'s result includes the authorization entries the
invocation needs (every `Address` parameter the contract calls
`require_auth`/`require_auth_for_args` on — for these contracts, that is
always the donor for vault writes and the NGO owner or an approver for
registry writes). Each entry must be signed by the account it names before
submission:

```ts
import { authorizeEntry } from '@stellar/stellar-sdk';

const signedAuthEntries = await Promise.all(
  simulated.result!.auth.map((entry) =>
    authorizeEntry(
      entry,
      donorKeypair, // the signer for the address this entry names
      simulated.latestLedger + 60, // validUntilLedgerSeq
      'Test SDF Network ; September 2015',
    ),
  ),
);
```

`rpc.assembleTransaction` already attaches these when you pass the
simulation result through as above; the explicit `authorizeEntry` call is
only needed when a different party signs the auth entry than the one
building and submitting the transaction (for example, a relayer submitting
on the donor's behalf). In that case, re-simulate with the signed entries
attached before building the final transaction, since adding signatures
changes the transaction's resource footprint.

## 3. Submit and poll

```ts
const signedTx = TransactionBuilder.fromXDR(
  prepared.toXDR(),
  'Test SDF Network ; September 2015',
);
signedTx.sign(donorKeypair);

const sendResult = await server.sendTransaction(signedTx);
if (sendResult.status !== 'PENDING') {
  throw new Error(`Submission failed: ${sendResult.status}`);
}

let result = await server.getTransaction(sendResult.hash);
while (result.status === rpc.Api.GetTransactionStatus.NOT_FOUND) {
  await new Promise((r) => setTimeout(r, 1000));
  result = await server.getTransaction(sendResult.hash);
}
```

## 4. Reading errors

Both contracts return `Result<_, Error>`. A failed invocation's simulation
or transaction result carries the numeric code (e.g. `Error(6)`), not the
variant name — map it back using the tables in the
[README](../README.md#error-codes) (`donation-vault` codes 1–38,
`ngo-registry` codes 1–15). Build a lookup once per contract rather than
hardcoding numbers inline, so a future append to the table only needs
updating in one place:

```ts
const DONATION_VAULT_ERRORS: Record<number, string> = {
  4: 'InvalidAmount',
  5: 'NothingToWithdraw',
  6: 'ContractPaused',
  18: 'NgoNotVerified',
  // ...full table in README.md#error-codes
};

function decodeVaultError(code: number): string {
  return DONATION_VAULT_ERRORS[code] ?? `UnknownError(${code})`;
}
```

A code's *meaning* never changes once shipped (see
[UPGRADE_POLICY.md §1](UPGRADE_POLICY.md#1-error-codes-are-append-only)),
so a lookup table built against today's README stays correct release over
release — it just needs new entries appended, never edited in place.

## 5. Events

Subscribe with `getEvents` (polling) or a hosted indexer, filtering by
contract ID and topic. Every event's first topic is its name as a `Symbol`
(see [EVENTS.md](EVENTS.md) for the full list and data shapes):

```ts
const events = await server.getEvents({
  startLedger: fromLedger,
  filters: [
    {
      type: 'contract',
      contractIds: [donationVaultContractId],
      topics: [['created']], // CreatedEvent — see EVENTS.md
    },
  ],
});
```

Events are the only reliable way to detect a code upgrade taking effect —
watch for `upgrade` (`UpgradedEvent`) on either contract, per
[UPGRADE_POLICY.md §4](UPGRADE_POLICY.md#4-announcing-breaking-changes).

## 6. Retries

- **Simulation failures** (`isSimulationError`) are not worth retrying
  blindly — they usually mean the call would fail on-chain too (e.g.
  `ContractPaused`, `NgoNotVerified`). Decode the error first; retry only
  for errors that are inherently transient (network/RPC errors, not a
  decoded contract error).
- **`sendTransaction` returning `TRY_AGAIN_LATER`** means the node's
  submission queue is full. Back off and resubmit the same signed envelope
  — do not rebuild and re-sign, which would change the transaction hash and
  could result in two submissions racing.
- **`getTransaction` returning `NOT_FOUND`** while polling is normal until
  the ledger closes; poll on a fixed interval (1–2s) up to the
  transaction's `timeBounds`, then treat it as expired and resubmit with a
  fresh simulation (fees and ledger sequence may have moved).
- Never resubmit a transaction that already returned `SUCCESS` or a
  decoded contract `Error` — both are final; the first a completed
  invocation, the second a client-side issue the retry would repeat.
