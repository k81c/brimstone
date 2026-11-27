/*---
description: >
  Annex B permits duplicate plain function declarations in a block, but not duplicate async,
  generator, or mixed declarations.
flags: [noStrict]
---*/

// Duplicate plain function declarations are accepted; the last one wins.
{
  function plain() {
    return 1;
  }
  function plain() {
    return 2;
  }
}
assert.sameValue(plain(), 2);

var invalid = [
  '{ async function f() {} async function f() {} }',
  '{ function* f() {} function* f() {} }',
  '{ function f() {} async function f() {} }',
  '{ async function f() {} function f() {} }',
  '{ function f() {} function* f() {} }',
  '{ function* f() {} function f() {} }',
];

for (var source of invalid) {
  assert.throws(SyntaxError, function () {
    eval(source);
  }, source);
}
