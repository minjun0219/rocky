import { GlobalRegistrator } from '@happy-dom/global-registrator';

GlobalRegistrator.register();

// happy-dom 의 WebSocket 은 진짜로 접속을 시도하고(테스트엔 데몬이 없다) 그 실패가 처리되지 않은 에러로 샌다.
// 지금 웹은 소켓을 쓰지 않지만(노트 편집기 · 보드 싱크는 2026-11-13 걷었다) 새 코드가 실수로 열어도 테스트가 새지 않게 걷어 둔다.
delete (globalThis as { WebSocket?: unknown }).WebSocket;
