/** Generated from the Rust Utoipa schema via xtask. Do not edit. */
import type { components, operations } from '../schema'

export interface Schemas2 {
ProfileView: {
            canonical_user_id?: string | null;
            display_name?: string | null;
        };
/** @enum {string} */
        ProgressActivity: "scratchpad" | "memory" | "skill" | "connector" | "search" | "browser" | "files" | "shell" | "sql" | "calendar" | "mail" | "image" | "audio_input" | "audio_output" | "subagent" | "sessions" | "configuration" | "model" | "unknown";
ProgressDelta: {
            at: string;
            attempt_id: string;
            items: components["schemas"]["ProgressItem"][];
            replace_kind?: string | null;
        };
ProgressItem: {
            activity?: null | components["schemas"]["ProgressActivity"];
            /** Format: int64 */
            elapsed_ms?: number | null;
            id: string;
            kind: string;
            label: string;
            started_at: string;
            state: string;
            updated_at: string;
        };
ProgressSnapshot: {
            attempt_id: string;
            items: components["schemas"]["ProgressItem"][];
            started_at: string;
            truncated: boolean;
            updated_at: string;
        };
RefreshView: {
            csrf: string;
            /** Format: int64 */
            expires_at: number;
            id: string;
        };
RuntimeStatus: {
            error?: string | null;
            memory_configured: boolean;
            model_configured: boolean;
            model_ready: boolean;
            phase: string;
            runtime_epoch: string;
        };
SecretList: {
            items: string[];
        };
SecretWrite: {
            value: string;
        };
SelectedSkillView: {
            id: string;
            name: string;
            revision: string;
        };
SessionCreate: {
            temporary?: boolean;
            title?: string | null;
        };
SessionEvent: {
            attempt_id?: string | null;
            data: unknown;
            id: string;
            kind: string;
            runtime_epoch: string;
            session_id: string;
        };
SessionList: {
            has_more: boolean;
            items: components["schemas"]["SessionView"][];
            next_cursor?: string | null;
        };
SessionStart: {
            attachments?: string[];
            client_message_id: string;
            skills?: components["schemas"]["SkillSelection"][];
            temporary?: boolean;
            text: string;
        };
SessionStartView: {
            id: string;
            published: boolean;
            session_id: string;
            state: string;
            temporary: boolean;
        };
SessionState: {
            active_attempt?: null | components["schemas"]["AttemptView"];
            progress?: null | components["schemas"]["ProgressSnapshot"];
            runtime_epoch: string;
            session: components["schemas"]["SessionView"];
            status: string[];
            text?: null | components["schemas"]["TextSnapshot"];
        };
SessionUpdate: {
            archived?: boolean | null;
            kind?: string | null;
            promotion_key?: string | null;
            /** Format: int64 */
            revision: number;
            state?: string | null;
            title?: string | null;
        };
SessionView: {
            created_at: string;
            host: string;
            id: string;
            kind: string;
            /** Format: int64 */
            revision: number;
            state: string;
            title: string;
        };
/** @enum {string} */
        SetupPhase: "credentials" | "profile" | "model" | "memory" | "ready" | "runtime_error";
SetupView: {
            identity_required: boolean;
            memory_configured: boolean;
            model_configured: boolean;
            model_ready: boolean;
            phase: components["schemas"]["SetupPhase"];
        };
SkillImport: {
            data_base64: string;
            filename: string;
            replace?: boolean;
        };
SkillList: {
            items: components["schemas"]["SkillView"][];
        };
SkillSelection: {
            id: string;
            revision: string;
        };
SkillUpdate: {
            enabled: boolean;
            revision: string;
        };
SkillView: {
            command?: string | null;
            content?: string | null;
            description: string;
            enabled: boolean;
            id: string;
            name: string;
            owner_only: boolean;
            revision: string;
        };
SkillWrite: {
            content: string;
            revision?: string | null;
        };
SubscriptionApplyRequest: {
            generation: string;
        };
SubscriptionCatalogModel: {
            context_window?: number | null;
            default_reasoning_effort?: string | null;
            description?: string | null;
            input_modalities: string[];
            name: string;
            reasoning_efforts: string[];
            selectable: boolean;
            slug: string;
            unavailable_reason?: string | null;
        };
SubscriptionCatalogView: {
            generation: string;
            models: components["schemas"]["SubscriptionCatalogModel"][];
            revision: string;
        };
SubscriptionConnectionView: {
            connected: boolean;
            generation?: string | null;
            message?: string | null;
            phase: components["schemas"]["SubscriptionPhase"];
            preferred_model_id?: string | null;
            selected_model_id?: string | null;
            selected_slug?: string | null;
        };
SubscriptionLimitsView: {
            observed_at?: string | null;
            state: string;
            windows: components["schemas"]["SubscriptionWindow"][];
        };
/** @enum {string} */
        SubscriptionPhase: "disconnected" | "connected" | "applying" | "ready" | "apply_failed";
SubscriptionSelection: {
            catalog_revision: string;
            generation: string;
            prefer?: boolean;
            slug: string;
        };
SubscriptionWindow: {
            bucket: string;
            /** Format: int64 */
            duration_seconds: number;
            /** Format: double */
            remaining_percent: number;
            /** Format: int64 */
            resets_at: number;
        };
TelegramBindingView: {
            actor_id: string;
            chat_id: string;
            /** Format: int64 */
            created_at: number;
        };
TelegramConnectionList: {
            items: components["schemas"]["TelegramConnectionView"][];
        };
TelegramConnectionView: {
            bindings: components["schemas"]["TelegramBindingView"][];
            bot_username?: string | null;
            connected: boolean;
            connector_id: string;
            requires_restart: boolean;
        };
TelegramCredentialWrite: {
            bot_token: string;
        };
TelegramLoginTokenView: {
            /** Format: int64 */
            expires_at: number;
            id: string;
            token: string;
        };
TelegramTokenRequest: Record<string, never>;
TextDelta: {
            attempt_id: string;
            generation: string;
            message_ids: string[];
            offset: number;
            operation: string;
            text: string;
        };
}
