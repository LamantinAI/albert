/** Generated from the Rust Utoipa schema. Do not edit. */
import type { components, operations } from '../schema'

export interface Paths5 {
"/sessions/{id}/messages/{request_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get sessions messages receipt */
        get: operations["sessions_get_sessions_id_messages_request_id"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/sessions/{id}/state": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get sessions state */
        get: operations["sessions_get_sessions_id_state"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/skill-imports": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Create skill imports */
        post: operations["skills_post_skill_imports"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/skills": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get skills */
        get: operations["skills_get_skills"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/skills/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get skills */
        get: operations["skills_get_skills_id"];
        /** Update skills */
        put: operations["skills_put_skills_id"];
        post?: never;
        /** Remove skills */
        delete: operations["skills_delete_skills_id"];
        options?: never;
        head?: never;
        /** Update skills */
        patch: operations["skills_patch_skills_id"];
        trace?: never;
    };
"/users": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get users */
        get: operations["users_list"];
        put?: never;
        /** Create users */
        post: operations["users_create"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
"/users/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /** Update users */
        patch: operations["users_update"];
        trace?: never;
    };
}
