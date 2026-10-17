/**
 * Workers 런타임 모듈 `cloudflare:workers`에서 `worker.ts`가 쓰는 것만. `@cloudflare/workers-types`는 DOM 타입과 겹쳐
 * 루트 tsconfig 하나로는 쓰지 못하고, 쓰는 것은 기반 클래스 하나라 의존성을 더하지 않는다.
 */
declare module 'cloudflare:workers' {
  /** Durable Object 기반 클래스. 런타임이 `ctx`(DurableObjectState)와 `env`를 넘긴다. 쓰는 모양은 하위 클래스가 적는다. */
  export abstract class DurableObject<Env = unknown> {
    protected readonly ctx: unknown;
    protected readonly env: Env;
    constructor(ctx: unknown, env: Env);
  }
}
