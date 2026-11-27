/*---
description: >
  An unparenthesized optional chain in a `new` callee is a SyntaxError in module code too.
flags: [module]
---*/

var invalid = [
  'new a?.b()',
  'new a.b?.c()',
  'new a?.[b]()',
  'new a?.()',
];

for (var source of invalid) {
  assert.throws(SyntaxError, function () {
    eval(source);
  }, source);
}

function Ctor(value) {
  this.value = value;
}

var a = { b: Ctor };
assert.sameValue(new (a?.b)(7).value, 7);
