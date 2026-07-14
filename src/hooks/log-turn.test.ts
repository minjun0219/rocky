import { describe, expect, it } from 'bun:test';
import { shouldCapture } from './log-turn';

describe('shouldCapture', () => {
  it('defaults to true when nothing is set', () => {
    expect(shouldCapture({}, undefined)).toBe(true);
    expect(shouldCapture({}, {})).toBe(true);
  });
  it('config.autoCapture:false disables', () => {
    expect(shouldCapture({}, { autoCapture: false })).toBe(false);
  });
  it('env=0/false/off/no disables (wins over config true)', () => {
    for (const v of ['0', 'false', 'off', 'no']) {
      expect(shouldCapture({ ROCKY_WORKLOG_AUTO_CAPTURE: v }, { autoCapture: true })).toBe(false);
    }
  });
  it('env other value enables (wins over config false)', () => {
    expect(shouldCapture({ ROCKY_WORKLOG_AUTO_CAPTURE: '1' }, { autoCapture: false })).toBe(true);
  });
});
