//! A simple intercept hook that discards odd numbers from all counter sources

use kiwi_sdk::hook;
use kiwi_sdk::types::intercept::{Action, Context, CounterEventCtx, EventCtx};

/// You must use the `#[hook::intercept]` attribute to define an intercept hook.
#[hook::intercept]
fn handle(ctx: Context) -> Action {
    match ctx.event {
        // We only care about counter sources in this example
        EventCtx::Counter(CounterEventCtx {
            source_id: _,
            count,
        }) => {
            if count % 2 == 0 {
                // Returning `Action::Forward` instructs Kiwi to forward the event
                // to the associated client.
                return Action::Forward;
            } else {
                // Returning `Action::Discard` instructs Kiwi to discard the event,
                // preventing it from reaching the associated client.
                return Action::Discard;
            }
        }
        _ => {}
    }

    Action::Forward
}
