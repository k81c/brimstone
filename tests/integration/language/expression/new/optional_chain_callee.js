/*---
description: >
  An unparenthesized optional chain is not allowed in the callee of a `new` expression, while
  parenthesized chains, computed keys and call arguments remain valid.
---*/

var invalid = [
  'new a?.b()',
  'new a?.b;',
  'new a.b?.c()',
  'new a?.[b]()',
  'new a?.()',
  'new f?.g;',
];

for (var source of invalid) {
  assert.throws(SyntaxError, function () {
    eval(source);
  }, source);
}

function Ctor(value) {
  this.value = value;
}

var a = { b: Ctor, d: Ctor };
a.b.c = Ctor;
var b = { c: 'd' };
var c = { d: 5 };
var f = Ctor;

assert.sameValue(new (a?.b)(1).value, 1);
assert.sameValue(new (a?.b).c(2).value, 2);
assert.sameValue(new (a?.b)['c'](3).value, 3);
// An array literal is a valid (but non-constructible) callee; only parsing matters here.
assert.throws(TypeError, function () {
  new [a?.b]();
});
assert.sameValue(new a[b?.c](4).value, 4);
assert.sameValue(new a[(b?.c)](5).value, 5);
assert.sameValue(new f(c?.d).value, 5);
