/**
 * core public surface — the plugin entry (`../index`) and the Stop hook (`../hooks/log-turn`)
 * consume these modules. worklog is the only tool domain left in TypeScript; it moves into the
 * Rust daemon next (see AGENTS.md → Scope), at which point this barrel goes with it.
 */
export * from './rocky-config';
export * from './worklog';
export * from './worklog-handlers';
