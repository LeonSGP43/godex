use std::sync::Arc;

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::codex::TurnContext;
use crate::codex::run_turn;
use crate::session_startup_prewarm::SessionStartupPrewarmResolution;
use crate::state::TaskKind;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::user_input::UserInput;
use tracing::Instrument;
use tracing::trace_span;

use super::SessionTask;
use super::SessionTaskContext;

const SUBAGENT_REGULAR_TASK_STACK_SIZE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct RegularTask;

impl RegularTask {
    pub(crate) fn new() -> Self {
        Self
    }
}

async fn send_turn_event(
    session: Arc<crate::codex::Session>,
    ctx: Arc<TurnContext>,
    event: EventMsg,
) {
    session.send_event_owned(ctx, event).await;
}

async fn set_reasoning_included(session: Arc<crate::codex::Session>, included: bool) {
    session.set_server_reasoning_included_owned(included).await;
}

async fn has_pending_input(session: Arc<crate::codex::Session>) -> bool {
    session.has_pending_input_owned().await
}

async fn turn_started_at(ctx: Arc<TurnContext>) -> Option<i64> {
    ctx.turn_timing_state.started_at_unix_secs().await
}

async fn consume_startup_prewarm(
    session: Arc<crate::codex::Session>,
    cancellation_token: CancellationToken,
) -> SessionStartupPrewarmResolution {
    session
        .consume_startup_prewarm_for_regular_turn_owned(cancellation_token)
        .await
}

async fn regular_task_run(
    session: Arc<SessionTaskContext>,
    ctx: Arc<TurnContext>,
    input: Vec<UserInput>,
    cancellation_token: CancellationToken,
) -> Option<String> {
    let sess = session.clone_session();
    let run_turn_span = trace_span!("run_turn");
    // Regular turns emit `TurnStarted` inline so first-turn lifecycle does
    // not wait on startup prewarm resolution.
    let event = EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: ctx.sub_id.clone(),
        started_at: turn_started_at(Arc::clone(&ctx)).await,
        model_context_window: ctx.model_context_window(),
        collaboration_mode_kind: ctx.collaboration_mode.mode,
    });
    send_turn_event(Arc::clone(&sess), Arc::clone(&ctx), event).await;
    set_reasoning_included(Arc::clone(&sess), /*included*/ false).await;
    let prewarmed_client_session =
        match consume_startup_prewarm(Arc::clone(&sess), cancellation_token.clone()).await {
            SessionStartupPrewarmResolution::Cancelled => return None,
            SessionStartupPrewarmResolution::Unavailable { .. } => None,
            SessionStartupPrewarmResolution::Ready(prewarmed_client_session) => {
                Some(*prewarmed_client_session)
            }
        };
    let mut next_input = input;
    let mut prewarmed_client_session = prewarmed_client_session;
    loop {
        let last_agent_message = run_turn(
            Arc::clone(&sess),
            Arc::clone(&ctx),
            next_input,
            prewarmed_client_session.take(),
            cancellation_token.child_token(),
        )
        .instrument(run_turn_span.clone())
        .await;
        if !has_pending_input(Arc::clone(&sess)).await {
            return last_agent_message;
        }
        next_input = Vec::new();
    }
}

async fn run_regular_task_on_dedicated_thread(
    session: Arc<SessionTaskContext>,
    ctx: Arc<TurnContext>,
    input: Vec<UserInput>,
    cancellation_token: CancellationToken,
) -> Option<String> {
    let (tx, rx) = oneshot::channel();
    let spawn_result = std::thread::Builder::new()
        .name("subagent-regular-task".to_string())
        .stack_size(SUBAGENT_REGULAR_TASK_STACK_SIZE_BYTES)
        .spawn(move || {
            let output = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()
                .and_then(|runtime| {
                    runtime.block_on(regular_task_run(session, ctx, input, cancellation_token))
                });
            let _ = tx.send(output);
        });

    if spawn_result.is_err() {
        return None;
    }

    rx.await.ok().flatten()
}

impl SessionTask for RegularTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    fn span_name(&self) -> &'static str {
        "session_task.turn"
    }

    fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> impl std::future::Future<Output = Option<String>> + Send {
        async move {
            let _ = self;
            run_regular_task_on_dedicated_thread(session, ctx, input, cancellation_token).await
        }
    }
}
