/** Generated from the Rust Utoipa schema. Do not edit. */
import type { components, operations } from '../schema'

export interface Paths4 {
"/prompts/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get prompts */
        get: operations["configuration_get_prompts_id"];
        /** Update prompts */
        put: operations["configuration_put_prompts_id"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/secrets": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get secrets */
        get: operations["configuration_get_secrets"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/secrets/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /** Update secrets */
        put: operations["configuration_put_secrets_id"];
        post?: never;
        /** Remove secrets */
        delete: operations["configuration_delete_secrets_id"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/session-starts": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Create session starts */
        post: operations["sessions_post_session_starts"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/session-starts/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get session starts */
        get: operations["sessions_get_session_starts_id"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/session-starts/{id}/cancellations": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Create session starts cancellations */
        post: operations["sessions_post_session_starts_id_cancellations"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/sessions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get sessions */
        get: operations["sessions_get_sessions"];
        put?: never;
        /** Create sessions */
        post: operations["sessions_post_sessions"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/sessions/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get sessions */
        get: operations["sessions_get_sessions_id"];
        put?: never;
        post?: never;
        /**
         * Delete an owned permanent conversation
         * @description Erases the conversation and its attachments. A revision is required. Retrying an owned deletion resumes pending cleanup; 204 means cleanup completed.
         */
        delete: operations["sessions_delete_session"];
        options?: never;
        head?: never;
        /** Update sessions */
        patch: operations["sessions_patch_sessions_id"];
        trace?: never;
    };
"/sessions/{id}/attachments/{asset_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get sessions attachments file */
        get: operations["attachments_read"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        /** Inspect sessions attachments file */
        head: operations["attachments_head"];
        patch?: never;
        trace?: never;
    };
"/sessions/{id}/compactions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Create sessions compactions */
        post: operations["sessions_post_sessions_id_compactions"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/sessions/{id}/events": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get sessions events */
        get: operations["sessions_subscribe"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/sessions/{id}/forks": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Fork a saved private conversation */
        post: operations["sessions_post_sessions_id_forks"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/sessions/{id}/messages": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get sessions messages */
        get: operations["sessions_get_sessions_id_messages"];
        put?: never;
        /** Create sessions messages */
        post: operations["sessions_post_sessions_id_messages"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
}
