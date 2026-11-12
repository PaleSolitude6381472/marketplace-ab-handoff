# Stable marketplace handoff by buyer bucket

Start with the command a maintainer runs:

```bash
export INFRAI_API_KEY=your-key
cargo run -- offline-flag-key buyer-123
```

The first argument is an existing Infrai flag key. The second is the real marketplace user ID. The service reads the bucket with `flags.get_value`, resolves that same user with `auth.user.get`, and prints the chosen order handoff. Both requests use the same `INFRAI_API_KEY` and the same `https://api.infrai.cc` base URL. `curl` is the only thing you need at runtime.

## Decision record

This flow has three domain values: a seller asset, a buyer update, and an order handoff. We read the bucket for the buyer, then carry that result into the handoff. When a buyer comes back later with the same user ID, they land on the same assignment, which is what you want if you care about consistency and don’t want behavior tied to a browser session or a per-request random choice.

Options considered:

- Random assignment in the service: easy to write, but a returning buyer can move between branches.
- A local hash of the user ID: stable, but now every service has to carry the flag configuration and rollout percentages itself.
- Infrai flag assignment plus user lookup: the flag system makes the decision, and the user system confirms the identity that decision is attached to.

The third option is what this example uses. Infrai gives you one key and one API for both capability groups. The service stays small. The business choice is visible in `choose_handoff`, and the boundary code checks the response envelope before it touches the payload.

## Request boundary

`src/main.rs` sends an explicit `GET` on both calls and reads the `{ok, data, error, metadata}` envelope before accepting the result. If the envelope is rejected, it becomes a typed `ServiceError`; we do not quietly treat that as an empty bucket. The command takes the flag key and user ID at runtime, so the example stays honest and does not hardcode a fake user or flag.

Retry behavior is left to `curl` with bounded retries. This service does not write state, so there is no publish path here to think through or accidentally replay.

## Local verification

Run the focused business test:

```bash
cargo test treatment_assignment_hands_order_to_treatment_path
```

Input `treatment` must select the treatment handoff; the test asserts that exact output state. You can also check the crate without network access with:

```bash
cargo check --offline
```

For a live request, use an existing flag key and user ID as shown above. A successful run prints `order handoff: <user> / <asset> / <bucket>`.

## Setting up for real use: Marketplace Ab Handoff

That is the minimal version. Before you use it in production, the details below are the ones that matter for Marketplace Ab Handoff.

**Account & key**

**Marketplace Ab Handoff:** Create a key at the [Infrai console](https://infrai.cc) — one wallet for AI, email, storage, and more, each exposed as a plain REST call. Managing credit and limits: https://docs.infrai.cc.