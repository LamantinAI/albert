import type { SelectedSkill } from "./composer/types";
import type { Attachment } from "./media/types";
import { useEffect, useRef, useState } from "react";
import {
  useLocation,
  useNavigate,
  useMatch,
  useSearchParams,
} from "react-router-dom";

import { api, ApiError, errorMessage, requestId } from "../../api/client";
import { uiPath } from "../../api/paths";
import { useClientContext } from "../../app/context";
import { Notice } from "../../components/ui";
import { Conversation } from "./Conversation";
import { useSessionDirectory } from "./SessionDirectory";
import { StartingConversation } from "./StartingConversation";
import type { components } from "../../api/schema";
import { Welcome } from "./Welcome";

export function Chats() {
  const id = useMatch("/chats/:id")?.params.id;
  const startId = useMatch("/chats/start/:startId")?.params.startId;
  const navigate = useNavigate();
  const location = useLocation();
  const welcomeKey = useRef(location.key);
  if (!id && !startId) welcomeKey.current = location.key;
  const [search] = useSearchParams();
  const { context } = useClientContext();
  const directory = useSessionDirectory();
  const [error, setError] = useState("");
  const [run, setRun] = useState<
    components["schemas"]["SessionStartView"] | null
  >(null);
  const [request, setRequest] = useState<{
    id: string;
    text: string;
    temporary: boolean;
    attachments?: Attachment[];
    skills?: SelectedSkill[];
  } | null>(null);
  const [canonicalReady, setCanonicalReady] = useState(false);
  const restoreDraft = useRef(false);
  const [entered, setEntered] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const generation = useRef(0);
  useEffect(
    () => () => {
      generation.current += 1;
    },
    [],
  );
  const inFlight = useRef(false);
  const pending = useRef<{
    id: string;
    text: string;
    temporary: boolean;
    attachments?: Attachment[];
    skills?: SelectedSkill[];
  } | null>(null);
  const effectiveId = id || (startId || request ? run?.session_id : undefined);
  const provisional = !id && !!effectiveId && !run?.published;
  const recover = async (value: string) => {
    const result = await api("/session-starts/{id}", "get", {
      path: { id: value },
    });
    return result;
  };
  useEffect(() => {
    if (!startId) return;
    let stopped = false;
    let busy = false;
    const poll = async () => {
      if (busy) return;
      busy = true;
      try {
        const result = await api("/session-starts/{id}", "get", {
          path: { id: startId },
        });
        if (stopped) return;
        setRun(result);
        setError("");
        if (result.published && canonicalReady) {
          const published = await directory.refreshSession(
            result.session_id,
            true,
          );
          if (stopped || !published) return;
          navigate(`/chats/${encodeURIComponent(result.session_id)}`, {
            replace: true,
          });
        }
      } catch (error) {
        if (!stopped && !inFlight.current) setError(errorMessage(error));
      } finally {
        busy = false;
      }
    };
    void poll();
    const timer = setInterval(() => void poll(), 500);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [startId, canonicalReady]);
  useEffect(() => {
    if (id) {
      generation.current += 1;
      inFlight.current = false;
      setSubmitting(false);
      setRun(null);
      setRequest(null);
      pending.current = null;
      setLeaving(false);
    } else if (!startId) {
      generation.current += 1;
      inFlight.current = false;
      setSubmitting(false);
      setLeaving(false);
      setEntered(false);
      if (restoreDraft.current) {
        restoreDraft.current = false;
        return;
      }
      setRun(null);
      setRequest(null);
      pending.current = directory.firstDraft?.unknown
        ? directory.firstDraft
        : null;
      setError("");
      setCanonicalReady(false);
    }
  }, [id, startId, location.key]);
  async function start(
    text: string,
    temporary: boolean,
    attachments: Attachment[] = [],
    skills: SelectedSkill[] = [],
  ) {
    if (inFlight.current) return false;
    const previous = pending.current;
    if (
      previous &&
      (previous.text !== text ||
        previous.temporary !== temporary ||
        JSON.stringify(previous.attachments ?? []) !==
          JSON.stringify(attachments) ||
        JSON.stringify(previous.skills ?? []) !== JSON.stringify(skills))
    ) {
      setError(
        "Confirm the previous message outcome before changing its text.",
      );
      return false;
    }
    const current = ++generation.current;
    const value = previous || {
      id: requestId(),
      text,
      temporary,
      attachments,
      skills,
    };
    pending.current = value;
    directory.setFirstDraft({ ...value, unknown: true });
    setError("");
    setRequest(value);
    setCanonicalReady(false);
    setSubmitting(true);
    setLeaving(true);
    inFlight.current = true;
    navigate(`/chats/start/${encodeURIComponent(value.id)}`, { replace: true });
    const delay = new Promise<void>((resolve) =>
      setTimeout(
        () => {
          if (current === generation.current) {
            setLeaving(false);
            setEntered(true);
          }
          resolve();
        },
        matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 180,
      ),
    );
    try {
      let result: components["schemas"]["SessionStartView"];
      try {
        result = await api("/session-starts", "post", {
          body: {
            client_message_id: value.id,
            text,
            temporary,
            skills: (value.skills ?? []).map(({ id, revision }) => ({
              id,
              revision,
            })),
            attachments: (value.attachments ?? []).map((a) => a.id),
          },
        });
      } catch (error) {
        try {
          result = await recover(value.id);
        } catch {
          throw error;
        }
      }
      if (current !== generation.current) return false;
      setRun(result);
      directory.setFirstDraft(null);
      await delay;
      if (current !== generation.current) return false;
      navigate(`/chats/start/${encodeURIComponent(value.id)}`, {
        replace: true,
      });
      return true;
    } catch (error) {
      await delay;
      if (current !== generation.current) return false;
      setRequest(null);
      setRun(null);
      restoreDraft.current = true;
      if (
        error instanceof ApiError &&
        [400, 403, 413, 422].includes(error.status)
      )
        pending.current = null;
      directory.setFirstDraft({ ...value, unknown: pending.current !== null });
      setError(errorMessage(error));
      navigate(`/chats?new=${temporary ? "temporary" : "permanent"}`, {
        replace: true,
      });
      return false;
    } finally {
      if (current === generation.current) {
        inFlight.current = false;
        setSubmitting(false);
      }
    }
  }
  return (
    <div className="chat-layout">
      <div className="chat-stage" data-entered={entered || undefined}>
        {effectiveId && !leaving ? (
          <Conversation
            key={effectiveId}
            id={effectiveId}
            provisional={provisional}
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
