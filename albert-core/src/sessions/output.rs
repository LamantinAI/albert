use octo_history::SessionKind;

use super::{
    cleanup_scope_for, transport::OutputRetention, transport::OutputScope, SessionContext,
};

pub fn output_scope(context: &SessionContext, attempt_id: Option<String>) -> OutputScope {
    let ephemeral = context.session().kind == SessionKind::Temporary;
    let cleanup_scope = context.cleanup_scope().map(str::to_owned).or_else(|| {
        ephemeral.then(|| cleanup_scope_for(context.reply_route(), context.session_id()))
    });
    OutputScope {
        scope: OutputRetention {
            key: context.root_history_key(),
            epoch: context.scope_epoch().unwrap_or("permanent").into(),
            ephemeral,
            cleanup_scope,
        },
        attempt_id,
    }
}
