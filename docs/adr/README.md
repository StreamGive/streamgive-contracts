# Architecture decision records

An architecture decision record (ADR) is a short note that captures one
significant design decision: what the situation was, what was decided, and what
that costs. Code comments say *how* something works; an ADR says *why it was
built that way*, so a later change can weigh the original reasoning instead of
rediscovering it.

## Index

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](./0001-lazy-accrual-model.md) | Compute stream accrual lazily instead of writing it every second | Accepted |
| [0002](./0002-checked-arithmetic.md) | Use checked arithmetic and fail with `ArithmeticOverflow` rather than wrap or panic | Accepted |
| [0003](./0003-registry-check-in-vault.md) | The vault checks NGO verification itself, and only when money would move toward an NGO | Accepted |
| [0004](./0004-ttl-policy.md) | Extend storage TTLs eagerly on every state-changing call | Accepted |

## Adding a record

1. Copy [template.md](./template.md) to `NNNN-short-title.md`, using the next
   unused number.
2. Fill it in. Keep it to about a page; link to code, issues, and other docs
   instead of repeating them.
3. Add a row to the index above in the same pull request.

A record is never deleted or rewritten when the decision changes. Write a new
record, and set the old one's status to `Superseded by NNNN`. Small
corrections to wording are fine.

## Note on the first records

Records 0001 to 0004 were written after the fact, from the code, its comments,
and the existing docs. They describe what the contracts do today and the
reasons the code gives for it. If you were part of the original decision and
remember a reason or an alternative that is missing, please amend the record.
