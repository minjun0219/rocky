/**
 * `rocky tui` 의 진입 — 데몬의 보드를 터미널에 띄운다. 보통은 Rust CLI 가 포트·actor·보드를 정해 넘기고, 개발할 때는
 * `bun run tui -- --port 8699 --board rocky` 로 바로 띄운다.
 *
 *   --port N      데몬 포트(기본 `ROCKY_TODO_PORT` 또는 8636)
 *   --actor NAME  변경 기록에 남을 이름(기본 `ROCKY_TODO_ACTOR` 또는 웹과 같은 `logan`)
 *   --board KEY   처음 볼 보드(기본 첫 보드)
 */
import { render } from 'ink';
import { parseArgs } from 'node:util';
import { createApi, openStream } from './api';
import { App } from './App';
import { createTuiStore } from './store';

const { values } = parseArgs({
  args: process.argv.slice(2),
  options: {
    port: { type: 'string' },
    actor: { type: 'string' },
    board: { type: 'string' },
  },
});

const port = values.port ?? process.env.ROCKY_TODO_PORT ?? '8636';
const actor = values.actor ?? (process.env.ROCKY_TODO_ACTOR?.trim() || 'logan');
const options = { base: `http://127.0.0.1:${port}`, actor };

const api = createApi(options);
const store = createTuiStore(api, values.board);
void store.getState().load();

// 붙을 때마다 전부 다시 받는다 — 끊긴 동안의 변경은 오지 않는다. 첫 연결도 받는다: 위의 load 가 실패했거나(데몬이
// 늦게 뜸) 그 읽기와 구독 사이에 바뀐 것이 있을 수 있다. 로컬 데몬이라 한 번 더 읽는 값은 싸다.
const close = openStream(options, {
  onOpen: () => {
    store.getState().setConnected(true);
    void store.getState().load();
  },
  onEvents: (events) => store.getState().queueEvents(events),
  onClose: (retryInMs) => store.getState().setConnected(false, retryInMs),
});

const app = render(<App store={store} />, {
  alternateScreen: true,
  incrementalRendering: true,
});
// 다시 붙기 대기(setTimeout)가 남아 있으면 이벤트 루프가 안 끝난다 — 화면이 내려가면 바로 끝낸다.
void app.waitUntilExit().then(() => {
  close();
  process.exit(0);
});
