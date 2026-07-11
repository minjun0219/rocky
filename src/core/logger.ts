import pino from "pino";

/**
 * stdio MCP 서버는 stdout 을 JSON-RPC 전용으로 보호해야 하므로 모든 로그는 stderr 로
 * 흘려야 한다. pino 의 destination(2) 가 그 역할 — `console.log` 를 절대 쓰지 않는다.
 */
export type LogLevel =
  | "trace"
  | "debug"
  | "info"
  | "warn"
  | "error"
  | "fatal"
  | "silent";

let logger: pino.Logger | null = null;

export function initLogger(level: LogLevel = "info"): pino.Logger {
  logger = pino(
    {
      level,
      base: undefined,
      timestamp: pino.stdTimeFunctions.isoTime,
    },
    pino.destination(2),
  );
  return logger;
}

export function getLogger(): pino.Logger {
  if (!logger) {
    logger = initLogger("info");
  }
  return logger;
}
