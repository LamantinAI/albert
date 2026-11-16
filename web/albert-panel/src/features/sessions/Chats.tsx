import { api } from "../../api/client";
import { uiPath } from "../../api/paths";
import { Notice } from "../../components/ui";
import { Conversation } from "./Conversation";
import { StartingConversation } from "./StartingConversation";
import { Welcome } from "./Welcome";
import type { ChatsProps } from "./useChats";
import { useChats } from "./useChats";
export function Chats(props: ChatsProps) {
  const {
    id,
    startId,
    navigate,
    location,
    welcomeKey,
    search,
    context,
    directory,
    error,
    setError,
    run,
    setRun,
    request,
    setCanonicalReady,
    entered,
    leaving,
    submitting,
    generation,
    effectiveId,
    provisional,
    recover,
    start,
  } = useChats(props);
  return (
    <div className="chat-layout">
      <div className="chat-stage" data-entered={entered || undefined}>
        {effectiveId && !leaving ? (
          <Conversation
            key={effectiveId}
            id={effectiveId}
            provisional={provisional}
            deleting={directory.actions.deletingSession?.id === effectiveId}
            onReady={() => setCanonicalReady(true)}
            startingText={request?.text}
            onCancelStart={
              startId
                ? async () => {
                    const observed = generation.current;
                    await api("/session-starts/{id}/cancellations", "post", {
                      path: { id: startId },
                      body: {},
                    });
                    const result = await recover(startId);
                    if (observed === generation.current) setRun(result);
                  }
                : undefined
            }
            inDirectory={directory.sessions.some(
              (session) => session.id === effectiveId,
            )}
            onSessionChange={directory.updateKnown}
            onRemoved={() => {
              directory.removeSession(effectiveId);
              directory.consumeDraft(effectiveId);
              const current = window.location.pathname.replace(/\/+$/, "");
              const openingId = startId ?? request?.id;
              if (
                current !==
                  uiPath(`/chats/${encodeURIComponent(effectiveId)}`) &&
                (!openingId ||
                  current !==
                    uiPath(`/chats/start/${encodeURIComponent(openingId)}`))
              )
                return;
              setError("");
              navigate("/chats", { replace: true });
            }}
            initialText={
              directory.initialDraft?.session === effectiveId
                ? directory.initialDraft.text
                : undefined
            }
            autoSendInitial={
              directory.initialDraft?.session === effectiveId &&
              !directory.initialDraft.attempted
            }
            onInitialStarted={() => directory.markAttempted(effectiveId)}
            onInitialConsumed={() => directory.consumeDraft(effectiveId)}
          />
        ) : request && !leaving ? (
          <StartingConversation
            text={request.text}
            temporary={request.temporary}
          />
        ) : (
          <Welcome
            key={welcomeKey.current}
            initialTemporary={
              directory.firstDraft?.temporary ??
              search.get("new") === "temporary"
            }
            initialText={directory.firstDraft?.text}
            initialAttachments={directory.firstDraft?.attachments}
            initialSkills={directory.firstDraft?.skills}
            locked={directory.firstDraft?.unknown}
            policyKey={`${context?.account?.id}:${context?.runtime_epoch}`}
            onSend={start}
            creating={submitting}
            leaving={leaving}
            ready={!!context?.setup.model_ready && !!context?.caps.chat}
          />
        )}
      </div>
      {run?.state === "expired" && (
        <Notice>
          This temporary request expired. Start a new conversation to continue.
        </Notice>
      )}
      {error && (
        <div className="chat-error">
          <Notice kind="error">{error}</Notice>
        </div>
      )}
    </div>
  );
}
