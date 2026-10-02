import { GlobalRegistrator } from '@happy-dom/global-registrator';

GlobalRegistrator.register();

// happy-dom 의 WebSocket 은 진짜로 접속을 시도하고(테스트엔 데몬이 없다) 그 실패가 처리되지 않은 에러로 샌다.
// 노트 편집기는 소켓이 없으면 HTTP 로 가므로(`sharedNoteTransport`) 여기서 걷는다 — 소켓 판은 가짜 WebSocket 으로
// 따로 테스트한다(`notedoc.test.ts`).
delete (globalThis as { WebSocket?: unknown }).WebSocket;
