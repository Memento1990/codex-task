import test from 'node:test';
import assert from 'node:assert/strict';
import { utilityKind, utilityKinds } from '../src/utility-window.ts';

test('utility boot accepts only exact function identifiers', () => {
  for (const kind of utilityKinds) assert.equal(utilityKind(kind), kind);
  assert.equal(utilityKind('maintenance'), 'maintenance');assert.equal(utilityKind('repair'),null);
  for (const value of [undefined, null, {}, 'main', 'toast', 'settings-extra', 'utility-settings', 'repair-extra', 'utility-repair', '../settings']) assert.equal(utilityKind(value), null);
});
