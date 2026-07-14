# Model Refinement Mapping

<!-- constrained-by ../getting-started/development.md#model-checking -->

Stateright models in [`crates/cfwdon-models`](../../crates/cfwdon-models) explore finite abstract state spaces. Production code in [`crates/cfwdon-worker`](../../crates/cfwdon-worker) carries richer state (D1 rows, HTTP auth, federation I/O). A **refinement mapping** links the two:

1. **Abstraction** — project concrete observable facts onto the model state.
2. **Domain step** — pure function in [`cfwdon-domain`](../../crates/cfwdon-domain) that both the model and worker should call.
3. **Worker guard** — extra preconditions the worker enforces before calling the domain step (auth, row shape, 404 masking). When a guard rejects, the worker **stutters** (no observable change).

Executable checks live in [`refinement`](../../crates/cfwdon-models/src/refinement/mod.rs) and run from `verify_models()`.

## Refinement relation

For observable state `c`, worker action `w`, and matching model action `a`:

- If the worker guard allows `w` on `c`, then `α(c') = δ(α(c), a)` where `c'` is the worker result and `δ` is the model transition.
- If the guard rejects, `c' = c`.

The worker is therefore a **restricted** implementation of the abstract model: it may refuse steps the model still explores, but it must not invent transitions the domain forbids.

## Catalog

The static catalog [`REFINEMENT_CATALOG`](../../crates/cfwdon-models/src/refinement/catalog.rs) lists all fifteen models with domain modules and worker call sites. Models marked `(pending worker wiring)` have domain helpers extracted but no worker delegation yet.

| Model | Domain module | Worker sites | Executable refinement |
| --- | --- | --- | --- |
| `quote` | `cfwdon_domain::quote` | `statuses/mutations`, publish intent | catalog only |
| `quote_approval` | `cfwdon_domain::quote` | `quote_owner_action_response`, remote upsert | yes |
| `registration_transition_events` | `account::registration` | `validate_account_registration_request` | yes |
| `status_draft_transition_events` | `status::draft` | `request_parsing`, `insert_status` | yes |
| `inbox_replay` | `federation::inbox` | `inbox/activity_store.rs`, `inbox.rs` | yes |
| `outbound_follow` | `delivery` | `delivery.rs`, `outbound_state.rs`, inbox follow handlers | yes |
| `concurrent_delivery` | `delivery` | `delivery.rs` | yes |
| `outbox_delivery_pool` | `delivery` | `delivery.rs` | yes |
| `outbox_pipeline` | `delivery` | `delivery.rs` | yes |
| `local_follow_request` | `follow` | `follow_requests.rs` | yes |
| `activitypub_visibility` | `remote::activitypub` | `activitypub/objects.rs`, `parse.rs` | yes |
| `status_draft_publish` | `status::draft` | `request_parsing`, `mutations` | catalog only |
| `registration_pipeline` | `account::registration` | `meta_placeholder_routes` | catalog only |
| `access_provision` | `account::registration` | `meta_placeholder_routes` | catalog only |
| `federation_request_policy` | `federation` | `http/request_validation`, `url_guard` | yes |
| `federation_dns_policy` | `federation::dns` | `url_guard.rs` | yes |

## Worked example: quote approval

<!-- derived-from #refinement-relation -->

Abstract state tracks `quote_state`, quote-target facts, and policy inputs — the same fields persisted on a status row.

| Model action | Domain call | Worker call | Worker guard |
| --- | --- | --- | --- |
| `RemoteUpsert` | `merged_quote_state_for_remote_upsert` | `remote/store.rs` previous-row merge | previous remote row exists |
| `OwnerApprove` | `quote_state_after_owner_approve` | `quote_owner_action_response(Approve)` | quote `pending` and `quote_of_uri` matches target |
| `OwnerReject` | `quote_state_after_owner_reject` | `quote_owner_action_response(Reject)` | same as approve |
| `Revoke` | `quote_state_after_revoke` | `revoke_quote_response` | requester is quote author |

[`check_quote_approval_refinement`](../../crates/cfwdon-models/src/refinement/quote_approval.rs) verifies:

- Model `next_state` matches domain helpers for every reachable abstract state.
- Worker guards are subsets of model actions; allowed steps match domain effects.

## Verification

```sh
cargo test -p cfwdon-models
```

`verify_models()` runs Stateright property checks and `refinement::verify_refinements()`.

## Extending a mapping

<!-- derived-from #catalog -->

1. Add or update an entry in `REFINEMENT_CATALOG` with domain symbols and worker paths.
2. If the worker now delegates to domain helpers, add `refinement/<model>.rs` with:
   - an `Observable` struct for persisted/API-visible facts;
   - `worker_allows` mirroring handler preconditions;
   - `assert_model_matches_domain` and/or `assert_worker_refinement` tests.
3. Register the check in `refinement::verify_refinements()`.
4. Update the **Executable refinement** column in this document.
