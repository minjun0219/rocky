/**
 * 중계소 테스트 도구. DO의 SQLite는 `bun:sqlite`로, 소켓은 받은 프레임을 모으는 가짜로 흉내 낸다.
 */
import { Database } from 'bun:sqlite';
import type { Peer, PeerState } from './relay';
import type { RelayStorage, SqlValue } from './storage';

export function memoryStorage(): RelayStorage {
  const db = new Database(':memory:');
  return {
    sql: {
      exec<T>(query: string, ...bindings: SqlValue[]) {
        const rows = db.query(query).all(...(bindings as string[])) as T[];
        return { toArray: () => rows };
      },
    },
    transactionSync: <T>(fn: () => T): T => db.transaction(fn)(),
  };
}

/** 받은 프레임을 모으는 소켓. 첨부는 DO 처럼 직렬화해 둔다. */
export class FakePeer implements Peer {
  sent: Record<string, unknown>[] = [];
  closed: { code: number; reason: string } | null = null;
  private attachment: string | null = null;

  send(text: string): void {
    if (this.closed) {
      throw new Error('socket closed');
    }
    this.sent.push(JSON.parse(text) as Record<string, unknown>);
  }

  close(code: number, reason: string): void {
    this.closed = { code, reason };
  }

  state(): PeerState | null {
    return this.attachment === null ? null : (JSON.parse(this.attachment) as PeerState);
  }

  setState(state: PeerState): void {
    this.attachment = JSON.stringify(state);
  }

  /** 지금까지 받은 프레임을 꺼내고 비운다. */
  take(): Record<string, unknown>[] {
    const out = this.sent;
    this.sent = [];
    return out;
  }
}
