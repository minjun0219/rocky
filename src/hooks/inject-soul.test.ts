import { describe, it, expect } from 'bun:test';
import { buildInjection } from './inject-soul';

describe('buildInjection', () => {
  it('emits SessionStart additionalContext when soul context present', () => {
    const out = buildInjection('PERSONA BODY');
    expect(out).not.toBeNull();
    const parsed = JSON.parse(out!);
    expect(parsed.hookSpecificOutput.hookEventName).toBe('SessionStart');
    expect(parsed.hookSpecificOutput.additionalContext).toBe('PERSONA BODY');
  });

  it('returns null when there is no soul context (vanilla)', () => {
    expect(buildInjection(null)).toBeNull();
  });
});
