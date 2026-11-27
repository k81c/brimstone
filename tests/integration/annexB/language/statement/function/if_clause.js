/*---
description: >
  Annex B B.3.3: a plain function declaration in an if/else clause is treated as the sole item of
  a synthetic block, with B.3.2 var leakage into the enclosing var scope.
flags: [noStrict]
---*/

var x = true;
if (x) function g() {}
assert.sameValue(typeof g, 'function');

if (false) function h() {}
assert.sameValue(typeof h, 'undefined');

if (true) function f1() {
  return 1;
} else function f2() {
  return 2;
}
assert.sameValue(f1(), 1);
assert.sameValue(typeof f2, 'undefined');

if (false) function f3() {
  return 3;
} else function f4() {
  return 4;
}
assert.sameValue(typeof f3, 'undefined');
assert.sameValue(f4(), 4);

if (true) function f5() {
  return 5;
} else function f5() {
  return 6;
}
assert.sameValue(f5(), 5);

if (true) if (true) function nested() {
  return 7;
} else function alt() {
  return 8;
}
assert.sameValue(nested(), 7);
assert.sameValue(typeof alt, 'undefined');

if (true) function cap() {
  return 42;
}
var getCap = () => cap();
assert.sameValue(getCap(), 42);

// No legacy var binding is created when a lexical declaration of the same name exists.
let q;
if (true) function q() {}
assert.sameValue(typeof q, 'undefined');

// The same semantics apply in a sloppy direct eval.
assert.sameValue(eval('if (true) function ef() { return 9 } typeof ef'), 'function');
assert.sameValue(eval('{ function bf() { return 8 } } typeof bf'), 'function');
