#!/usr/bin/env -S gjs -m
/* Unit tests for the extension's pure helpers.
 *
 * Deliberately covers only the gi-free module: everything else in the
 * extension needs a live GNOME Shell and is checked by the manual matrix in
 * doc/testing.md.
 *
 *   gjs -m extension/test/terms-test.js
 */

import {ellipsize, looksLikeTerm} from '../terms.js';

let failures = 0;

function check(description, actual, expected) {
    if (actual === expected) {
        print(`  ok    ${description}`);
    } else {
        print(`  FAIL  ${description}\n        expected ${expected}, got ${actual}`);
        failures += 1;
    }
}

print('looksLikeTerm');
for (const term of ['ephemeral', 'Ephemeral', 'ad hoc', 'by and large', 'rule of thumb', 'café'])
    check(`accepts "${term}"`, looksLikeTerm(term), true);

check('accepts a term with surrounding whitespace', looksLikeTerm('  ephemeral  '), true);
check('rejects null', looksLikeTerm(null), false);
check('rejects an empty string', looksLikeTerm(''), false);
check('rejects whitespace only', looksLikeTerm('   '), false);
check('rejects a full sentence',
    looksLikeTerm('The effects were ephemeral, and nobody noticed them at all.'), false);
check('rejects more than five words', looksLikeTerm('one two three four five six'), false);
check('accepts exactly five words', looksLikeTerm('one two three four five'), true);
check('rejects text over eighty characters', looksLikeTerm('a'.repeat(81)), false);
check('accepts exactly eighty characters', looksLikeTerm('a'.repeat(80)), true);
check('rejects multi-line text', looksLikeTerm('ephemeral\nubiquitous'), false);
check('rejects digits only', looksLikeTerm('12345'), false);
check('rejects punctuation only', looksLikeTerm('---'), false);
check('rejects a URL', looksLikeTerm('https://example.com'), false);

print('ellipsize');
check('leaves a short term alone', ellipsize('ephemeral', 18), 'ephemeral');
check('leaves an exactly-fitting term alone', ellipsize('ephemeral', 9), 'ephemeral');
check('shortens a long term', ellipsize('incomprehensibility', 10), 'incompreh…');
check('counts characters, not bytes', ellipsize('café café café', 6), 'café …');
check('never exceeds the budget', ellipsize('ephemeral', 1), '…');
check('the result fits maxChars exactly', ellipsize('ephemeral', 5).length, 5);

print('');
if (failures === 0) {
    print('all extension helper tests passed');
} else {
    print(`${failures} failed`);
    imports.system.exit(1);
}
