/** Generated from the Rust Utoipa schema via xtask. Do not edit. */
import type { components, operations } from '../schema'

export interface Schemas1 {
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
/** @enum {string} */
        SubscriptionPhase: "disconnected" | "connected" | "applying" | "ready" | "apply_failed";
SubscriptionSelection: {
            catalog_revision: string;
            generation: string;
            prefer?: boolean;
            slug: string;
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
TextSnapshot: {
            attempt_id: string;
            generation: string;
            message_ids: string[];
            offset: number;
            state: string;
            text: string;
            truncated: boolean;
        };
UserCreate: {
            password: string;
            role: components["schemas"]["AccountRole"];
            username: string;
        };
UserList: {
            items: components["schemas"]["AccountView"][];
        };
UserUpdate: {
            disabled?: boolean | null;
            /** Format: int64 */
            revision: number;
            role?: null | components["schemas"]["AccountRole"];
        };
ValidationView: {
            issues: string[];
            requires_restart: boolean;
            valid: boolean;
        };
}
