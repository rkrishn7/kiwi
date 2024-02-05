use kiwi_sdk::hook::intercept;
use kiwi_sdk::types::intercept::{Action, Context, CounterEventCtx, EventCtx};

#[intercept]
fn handle(ctx: Context) -> Action {
    match ctx.event {
        EventCtx::Counter(CounterEventCtx {
            source_id: _,
            count,
        }) => {
            if count % 2 == 0 {
                return Action::Forward;
            } else {
                return Action::Discard;
            }
        }
        _ => {}
    }

    Action::Forward
}
