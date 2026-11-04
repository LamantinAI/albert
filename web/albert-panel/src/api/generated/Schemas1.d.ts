/** Generated from the Rust Utoipa schema via xtask. Do not edit. */
import type { components, operations } from '../schema'

export interface Schemas1 {
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
